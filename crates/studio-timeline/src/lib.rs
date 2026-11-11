//! Canonical timeline model.
//!
//! Coordinate system: every timeline position is an integer [`Tick`] (1/705 600 000 s)
//! measured from timeline zero. A clip occupies `[start, start + duration)` on its track.
//! Its source time at timeline time `t` is `source_in + (t - start) * speed`.
//! Clips on one track never overlap; transitions are centred on the cut and consume
//! *handles* (unused source media) of both neighbouring clips, which validation checks.

pub mod effects;
pub mod history;
pub mod model;
pub mod ops;
pub mod validate;

pub use effects::*;
pub use history::History;
pub use model::*;
pub use validate::{validate, AssetInfo, ValidationIssue, ValidationSeverity};
