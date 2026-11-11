//! Cooperative cancellation and progress reporting, independent of any async runtime
//! so CPU-bound analysis code (run on blocking threads) can use it directly.

use crate::error::{StudioError, StudioResult};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    /// Returns `Err(Cancelled)` if cancellation was requested.
    pub fn check(&self) -> StudioResult<()> {
        if self.is_cancelled() {
            Err(StudioError::cancelled())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    /// 0.0 ..= 1.0, or `None` when indeterminate.
    pub fraction: Option<f32>,
    pub stage: String,
}

/// Receives progress updates. Must be cheap and thread-safe.
pub type ProgressSink = Arc<dyn Fn(Progress) + Send + Sync>;

pub fn noop_progress() -> ProgressSink {
    Arc::new(|_| {})
}

/// Maps a sub-task's 0..1 progress into a [lo, hi] window of a parent sink.
pub fn sub_progress(parent: &ProgressSink, lo: f32, hi: f32) -> ProgressSink {
    let parent = parent.clone();
    Arc::new(move |p: Progress| {
        parent(Progress {
            fraction: p.fraction.map(|f| lo + (hi - lo) * f.clamp(0.0, 1.0)),
            stage: p.stage,
        })
    })
}

pub fn report(sink: &ProgressSink, fraction: f32, stage: &str) {
    sink(Progress {
        fraction: Some(fraction.clamp(0.0, 1.0)),
        stage: stage.to_string(),
    })
}
