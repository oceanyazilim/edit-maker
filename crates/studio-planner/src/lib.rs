//! Creative Planning Engine.
//!
//! Converts (audio analysis, scene database, preset, controls, target, seed)
//! into a canonical [`studio_timeline::Timeline`]. The same inputs always yield
//! the same plan (deterministic RNG streams derived from the seed).
//!
//! See `docs/PLANNER.md` for the definition of every control.

pub mod controls;
pub mod niche;
pub mod plan;
pub mod presets;
pub mod segments;
pub mod similarity;

pub use controls::{CreativeControls, Variant};
pub use niche::{builtin_niches, NicheProfile};
pub use plan::{
    plan_edit, Cue, EditPlan, FootageSource, OutputTarget, PlanInput, PlannedSegment, ResolvedCue,
};
pub use presets::{builtin_presets, EditPreset};
pub use segments::{recommend_segments, SegmentKind, SegmentSuggestion};
pub use similarity::similarity;
