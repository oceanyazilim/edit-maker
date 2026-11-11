//! Project Core, Asset Library, analysis cache and Automation Engine.

pub mod discovery;
pub mod engine;
pub mod library;
pub mod model;
pub mod profile;
pub mod store;

pub use engine::{AppDirs, Engine};
pub use model::*;
pub use profile::{Profile, ProfileDefaults, DEFAULT_PROFILE_ID};
