//! Video Scene Intelligence.
//!
//! Input: a stream of small RGB24 frames at a constant analysis frame rate
//! (produced by `studio-media` through FFmpeg). Output: [`VideoAnalysis`] with
//! detected cuts, scenes and per-scene descriptors.
//!
//! Everything here is classical computer vision (no ML model): descriptors are
//! measurable signal properties – motion, brightness, sharpness … – and tags are
//! derived from those measurements only. No semantic claims (e.g. "close-up",
//! "character") are made without a validated model.

mod analyzer;
mod frame;
pub mod model;

pub use analyzer::{SceneAnalyzer, VisionConfig};
pub use model::*;
