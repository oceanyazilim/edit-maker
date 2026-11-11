use std::sync::{Arc, Mutex};

use studio_core::StudioResult;
use studio_jobs::JobManager;
use studio_project::{AppDirs, Engine, Project};
use studio_timeline::History;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub project: Arc<Mutex<Option<Project>>>,
    pub history: Arc<Mutex<History>>,
    pub jobs: JobManager,
}

impl AppState {
    pub fn new() -> StudioResult<AppState> {
        let engine = Arc::new(Engine::new(AppDirs::default_for_user())?);
        Ok(AppState {
            engine,
            project: Arc::new(Mutex::new(None)),
            history: Arc::new(Mutex::new(History::default())),
            jobs: JobManager::new(),
        })
    }
}
