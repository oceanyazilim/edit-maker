use serde::{Deserialize, Serialize};
use studio_core::{Rational, Tick};

pub const VISION_VERSION: u32 = 1;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CutKind {
    Hard,
    Gradual,
    /// Boundary created by a black segment (fade through black).
    Black,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CutEvent {
    pub t: Tick,
    pub kind: CutKind,
    pub score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub index: u32,
    pub start: Tick,
    pub end: Tick,
    /// Mean motion intensity, 0..1.
    pub motion: f32,
    /// Peak motion within the scene, 0..1.
    pub motion_peak: f32,
    /// Time of the strongest action moment (absolute source time).
    pub motion_peak_t: Option<Tick>,
    /// Additional motion spikes (absolute source times), strongest first.
    pub impacts: Vec<Tick>,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Relative sharpness 0..1 (Laplacian energy; resolution dependent).
    pub sharpness: f32,
    /// Fraction of near-black frames.
    pub black_ratio: f32,
    /// 1 = steady camera, 0 = very shaky.
    pub stability: f32,
    /// Mean global (camera) motion in normalized frame units per second (+x right, +y down).
    pub camera_dx: f32,
    pub camera_dy: f32,
    /// Normalized visual energy 0..1 (motion, contrast, colour).
    pub energy: f32,
    /// Technical quality 0..1 (sharpness, exposure, stability, black frames).
    pub quality: f32,
    /// Motion/detail-weighted focus point (normalized 0..1), used by smart crop.
    pub focus_x: f32,
    pub focus_y: f32,
    pub tags: Vec<String>,
    pub usable: bool,
}

impl Scene {
    pub fn duration(&self) -> Tick {
        self.end - self.start
    }
}

/// Compact per-frame curves for UI and diagnostics.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct FrameCurves {
    pub motion: Vec<f32>,
    pub brightness: Vec<f32>,
    pub cut_score: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoAnalysis {
    pub version: u32,
    pub analysis_fps: Rational,
    pub frame_count: u64,
    pub width: u32,
    pub height: u32,
    pub cuts: Vec<CutEvent>,
    pub scenes: Vec<Scene>,
    pub curves: FrameCurves,
}

impl VideoAnalysis {
    pub fn duration(&self) -> Tick {
        Tick::from_frame(self.frame_count as i64, self.analysis_fps)
    }
}
