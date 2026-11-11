//! Render Engine.
//!
//! Preview and export share one code path (ADR-0004):
//! [`state::clip_state`] evaluates the effect specification at a time, and
//! [`compositor`] turns decoded source frames + state into output pixels. The
//! export pipeline streams composited frames into an FFmpeg encoder; the preview
//! path composites single frames (or renders a low-resolution preview file) with
//! the very same functions.

pub mod audio_graph;
pub mod compositor;
pub mod encode;
pub mod pipeline;
pub mod sources;
pub mod state;
pub mod verify;

pub use pipeline::{render_frame, render_timeline, RenderReport, RenderSettings};
pub use sources::{RenderAsset, RenderAssetKind};
