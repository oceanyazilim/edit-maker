//! Job Scheduler.
//!
//! Jobs run on dedicated OS threads (CPU-bound analysis and FFmpeg supervision
//! never block the UI or an async runtime). Each job kind has a concurrency
//! limit (e.g. one render at a time). A panicking job is isolated and reported
//! as failed; it never takes down the application.

use std::collections::{HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use studio_core::{CancelToken, Progress, ProgressSink, StudioError, StudioResult};

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Import,
    AudioAnalysis,
    VideoAnalysis,
    Proxy,
    Generate,
    Render,
    Preview,
}

impl JobKind {
    pub fn default_limit(self) -> usize {
        match self {
            JobKind::Render => 1,
            JobKind::Preview => 1,
            JobKind::Generate => 1,
            JobKind::Proxy => 1,
            JobKind::VideoAnalysis => 2,
            JobKind::AudioAnalysis => 2,
            JobKind::Import => 4,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct JobInfo {
    pub id: String,
    pub kind: JobKind,
    pub label: String,
    /// Originating profile. Jobs keep running (and stay attributed) when the user switches profiles.
    pub owner: Option<String>,
    pub status: JobStatus,
    pub progress: Option<f32>,
    pub stage: String,
    pub error: Option<StudioError>,
    pub result: Option<serde_json::Value>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

pub type JobListener = Arc<dyn Fn(&JobInfo) + Send + Sync>;

struct Slot {
    info: JobInfo,
    cancel: CancelToken,
}

struct Inner {
    jobs: HashMap<String, Slot>,
    order: VecDeque<String>,
    running: HashMap<JobKind, usize>,
    limits: HashMap<JobKind, usize>,
}

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<(Mutex<Inner>, Condvar)>,
    listener: Arc<Mutex<Option<JobListener>>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(Inner {
                    jobs: HashMap::new(),
                    order: VecDeque::new(),
                    running: HashMap::new(),
                    limits: HashMap::new(),
                }),
                Condvar::new(),
            )),
            listener: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_limit(&self, kind: JobKind, limit: usize) {
        self.inner
            .0
            .lock()
            .unwrap()
            .limits
            .insert(kind, limit.max(1));
        self.inner.1.notify_all();
    }

    pub fn set_listener(&self, l: JobListener) {
        *self.listener.lock().unwrap() = Some(l);
    }

    fn emit(&self, info: &JobInfo) {
        if let Some(l) = self.listener.lock().unwrap().as_ref() {
            l(info);
        }
    }

    fn update<F: FnOnce(&mut JobInfo)>(&self, id: &str, f: F) -> Option<JobInfo> {
        let mut g = self.inner.0.lock().unwrap();
        let slot = g.jobs.get_mut(id)?;
        f(&mut slot.info);
        Some(slot.info.clone())
    }

    /// Submit a job. The closure receives a cancel token and a progress sink.
    pub fn submit<T, F>(&self, kind: JobKind, label: impl Into<String>, f: F) -> String
    where
        T: Serialize,
        F: FnOnce(CancelToken, ProgressSink) -> StudioResult<T> + Send + 'static,
    {
        self.submit_owned(None, kind, label, f)
    }

    /// Submit a job attributed to an owner (profile id).
    pub fn submit_owned<T, F>(
        &self,
        owner: Option<String>,
        kind: JobKind,
        label: impl Into<String>,
        f: F,
    ) -> String
    where
        T: Serialize,
        F: FnOnce(CancelToken, ProgressSink) -> StudioResult<T> + Send + 'static,
    {
        let id = format!("job_{}", uuid::Uuid::new_v4().simple());
        let cancel = CancelToken::new();
        let info = JobInfo {
            id: id.clone(),
            kind,
            label: label.into(),
            owner,
            status: JobStatus::Queued,
            progress: None,
            stage: "Queued".into(),
            error: None,
            result: None,
            created_at: now(),
            finished_at: None,
            elapsed_ms: None,
        };
        {
            let mut g = self.inner.0.lock().unwrap();
            g.jobs.insert(
                id.clone(),
                Slot {
                    info: info.clone(),
                    cancel: cancel.clone(),
                },
            );
            g.order.push_back(id.clone());
            // Keep history bounded.
            while g.order.len() > 500 {
                if let Some(old) = g.order.pop_front() {
                    if g.jobs
                        .get(&old)
                        .map(|s| s.info.status.is_finished())
                        .unwrap_or(true)
                    {
                        g.jobs.remove(&old);
                    } else {
                        g.order.push_back(old);
                        break;
                    }
                }
            }
        }
        self.emit(&info);
        let mgr = self.clone();
        let jid = id.clone();
        std::thread::Builder::new()
            .name(format!("job-{kind:?}"))
            .spawn(move || mgr.run(jid, kind, cancel, f))
            .expect("spawn job thread");
        id
    }

    fn run<T, F>(&self, id: String, kind: JobKind, cancel: CancelToken, f: F)
    where
        T: Serialize,
        F: FnOnce(CancelToken, ProgressSink) -> StudioResult<T> + Send + 'static,
    {
        // Wait for a free slot of this kind.
        {
            let (lock, cv) = &*self.inner;
            let mut g = lock.lock().unwrap();
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                let limit = *g.limits.get(&kind).unwrap_or(&kind.default_limit());
                let running = *g.running.get(&kind).unwrap_or(&0);
                if running < limit {
                    *g.running.entry(kind).or_insert(0) += 1;
                    break;
                }
                g = cv.wait_timeout(g, Duration::from_millis(200)).unwrap().0;
            }
        }
        if cancel.is_cancelled() {
            if let Some(i) = self.update(&id, |i| {
                i.status = JobStatus::Cancelled;
                i.stage = "Cancelled".into();
                i.finished_at = Some(now());
            }) {
                self.emit(&i);
            }
            return;
        }
        let start = Instant::now();
        if let Some(i) = self.update(&id, |i| {
            i.status = JobStatus::Running;
            i.stage = "Starting".into();
        }) {
            self.emit(&i);
        }
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let mgr = self.clone();
        let pid = id.clone();
        let sink: ProgressSink = Arc::new(move |p: Progress| {
            let info = mgr.update(&pid, |i| {
                i.progress = p.fraction;
                i.stage = p.stage.clone();
            });
            let mut le = last_emit.lock().unwrap();
            // Throttle UI events to ~10/s per job.
            if le.elapsed() >= Duration::from_millis(100) {
                *le = Instant::now();
                if let Some(i) = info {
                    mgr.emit(&i);
                }
            }
        });
        let res = std::panic::catch_unwind(AssertUnwindSafe(|| f(cancel.clone(), sink)));
        let elapsed = start.elapsed().as_millis() as u64;
        let info = self.update(&id, |i| {
            i.finished_at = Some(now());
            i.elapsed_ms = Some(elapsed);
            match res {
                Ok(Ok(v)) => {
                    i.status = JobStatus::Completed;
                    i.progress = Some(1.0);
                    i.stage = "Completed".into();
                    i.result = serde_json::to_value(v).ok();
                }
                Ok(Err(e)) if e.is_cancelled() => {
                    i.status = JobStatus::Cancelled;
                    i.stage = "Cancelled".into();
                }
                Ok(Err(e)) => {
                    tracing::warn!(job = %i.id, kind = ?i.kind, "job failed: {} {:?}", e.message, e.details);
                    i.status = JobStatus::Failed;
                    i.stage = "Failed".into();
                    i.error = Some(e);
                }
                Err(p) => {
                    let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
                    tracing::error!(job = %i.id, "job panicked: {msg}");
                    i.status = JobStatus::Failed;
                    i.stage = "Failed".into();
                    i.error = Some(StudioError::internal("An internal error occurred while processing.").with_details(msg));
                }
            }
        });
        {
            let (lock, cv) = &*self.inner;
            let mut g = lock.lock().unwrap();
            if let Some(r) = g.running.get_mut(&kind) {
                *r = r.saturating_sub(1);
            }
            cv.notify_all();
        }
        if let Some(i) = info {
            self.emit(&i);
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let g = self.inner.0.lock().unwrap();
        if let Some(s) = g.jobs.get(id) {
            s.cancel.cancel();
            true
        } else {
            false
        }
    }

    pub fn get(&self, id: &str) -> Option<JobInfo> {
        self.inner
            .0
            .lock()
            .unwrap()
            .jobs
            .get(id)
            .map(|s| s.info.clone())
    }

    pub fn list(&self) -> Vec<JobInfo> {
        let g = self.inner.0.lock().unwrap();
        g.order
            .iter()
            .filter_map(|id| g.jobs.get(id).map(|s| s.info.clone()))
            .collect()
    }

    /// Jobs of one owner (profile).
    pub fn list_for(&self, owner: &str) -> Vec<JobInfo> {
        self.list()
            .into_iter()
            .filter(|j| j.owner.as_deref() == Some(owner))
            .collect()
    }

    /// Block until the job finishes (tests / CLI).
    pub fn wait(&self, id: &str, timeout: Duration) -> Option<JobInfo> {
        let start = Instant::now();
        loop {
            let i = self.get(id)?;
            if i.status.is_finished() {
                return Some(i);
            }
            if start.elapsed() > timeout {
                return Some(i);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn completes_with_result_and_progress() {
        let m = JobManager::new();
        let events = Arc::new(AtomicUsize::new(0));
        let e2 = events.clone();
        m.set_listener(Arc::new(move |_| {
            e2.fetch_add(1, Ordering::SeqCst);
        }));
        let id = m.submit(JobKind::AudioAnalysis, "test", |_c, p| {
            p(Progress {
                fraction: Some(0.5),
                stage: "half".into(),
            });
            Ok(42)
        });
        let i = m.wait(&id, Duration::from_secs(5)).unwrap();
        assert_eq!(i.status, JobStatus::Completed);
        assert_eq!(i.result, Some(serde_json::json!(42)));
        assert!(events.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn cancellation_and_failure_isolation() {
        let m = JobManager::new();
        let id = m.submit(JobKind::Render, "long", |c, _p| -> StudioResult<()> {
            for _ in 0..500 {
                c.check()?;
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        std::thread::sleep(Duration::from_millis(30));
        assert!(m.cancel(&id));
        assert_eq!(
            m.wait(&id, Duration::from_secs(5)).unwrap().status,
            JobStatus::Cancelled
        );

        let id = m.submit(JobKind::Import, "panics", |_c, _p| -> StudioResult<()> {
            panic!("boom")
        });
        let i = m.wait(&id, Duration::from_secs(5)).unwrap();
        assert_eq!(i.status, JobStatus::Failed);
        assert!(i.error.unwrap().details.unwrap().contains("boom"));

        let id = m.submit(JobKind::Import, "fails", |_c, _p| -> StudioResult<()> {
            Err(StudioError::invalid_media("bad"))
        });
        assert_eq!(
            m.wait(&id, Duration::from_secs(5)).unwrap().status,
            JobStatus::Failed
        );
    }

    #[test]
    fn owner_is_retained() {
        let m = JobManager::new();
        let a = m.submit_owned(Some("profA".into()), JobKind::Render, "a", |_c, _p| Ok(()));
        let b = m.submit_owned(Some("profB".into()), JobKind::Import, "b", |_c, _p| Ok(()));
        m.wait(&a, Duration::from_secs(5));
        m.wait(&b, Duration::from_secs(5));
        assert_eq!(m.get(&a).unwrap().owner.as_deref(), Some("profA"));
        assert_eq!(m.list_for("profB").len(), 1);
        assert_eq!(m.list_for("profB")[0].id, b);
    }

    #[test]
    fn concurrency_limit() {
        let m = JobManager::new();
        let running = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let ids: Vec<String> = (0..4)
            .map(|_| {
                let r = running.clone();
                let mx = max_seen.clone();
                m.submit(JobKind::Render, "r", move |_c, _p| {
                    let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                    mx.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(40));
                    r.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .collect();
        for id in ids {
            m.wait(&id, Duration::from_secs(10));
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }
}
