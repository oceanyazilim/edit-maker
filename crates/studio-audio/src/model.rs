use serde::{Deserialize, Serialize};

/// Bump when the analysis algorithm changes in a way that invalidates caches.
pub const ANALYSIS_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnalysisConfig {
    /// 0 = only very clear kicks, 1 = permissive.
    pub kick_sensitivity: f32,
    /// Optional absolute override of the minimum kick confidence.
    pub kick_min_confidence: Option<f32>,
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// Centre of the tempo prior (log-normal, 1 octave std).
    pub prior_bpm: f64,
    /// Manual BPM override: skips estimation, still tracks phase.
    pub bpm_override: Option<f64>,
    pub beats_per_bar: u32,
    /// Higher = beats follow a rigid tempo more strictly.
    pub beat_tightness: f64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            kick_sensitivity: 0.5,
            kick_min_confidence: None,
            min_bpm: 60.0,
            max_bpm: 200.0,
            prior_bpm: 120.0,
            bpm_override: None,
            beats_per_bar: 4,
            beat_tightness: 100.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TempoEstimate {
    pub bpm: f64,
    /// Heuristic 0..1 (peak salience vs. alternatives), not calibrated.
    pub confidence: f32,
    /// Metrically related alternatives (half/double time) with relative salience.
    pub alternatives: Vec<TempoAlternative>,
    pub overridden: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TempoAlternative {
    pub bpm: f64,
    pub salience: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BeatEvent {
    pub sample: i64,
    /// Normalized onset strength at the beat (0..1).
    pub strength: f32,
    pub is_downbeat: bool,
    /// Bar index (0-based) once downbeats are assigned.
    pub bar: i32,
    /// Position inside the bar, 0-based.
    pub beat_in_bar: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PercussionEvent {
    pub sample: i64,
    /// Relative loudness within the track, 0..1.
    pub intensity: f32,
    /// Heuristic 0..1.
    pub confidence: f32,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Intro,
    Groove,
    BuildUp,
    Drop,
    Breakdown,
    Outro,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MusicalSection {
    pub start_sample: i64,
    pub end_sample: i64,
    pub kind: SectionKind,
    /// Relative energy 0..1 within the track.
    pub energy: f32,
    /// Change in energy across the section (positive = rising).
    pub energy_slope: f32,
    pub confidence: f32,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropKind {
    MainDrop,
    SecondaryDrop,
    FalseDropCandidate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DropFeatures {
    pub energy_contrast: f32,
    pub low_reentry: f32,
    pub buildup: f32,
    pub pre_drop_dip: f32,
    pub post_density: f32,
    pub sustained: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DropCandidate {
    pub sample: i64,
    pub kind: DropKind,
    /// Heuristic 0..1.
    pub score: f32,
    pub features: DropFeatures,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SilenceRegion {
    pub start_sample: i64,
    pub end_sample: i64,
}

/// Frame-rate feature curves for visualization (hop = `hop_samples`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AnalysisCurves {
    pub hop_samples: u32,
    pub rms_db: Vec<f32>,
    pub low_db: Vec<f32>,
    pub mid_db: Vec<f32>,
    pub high_db: Vec<f32>,
    pub onset: Vec<f32>,
    pub centroid_hz: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AudioSummary {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    /// 95th − 10th percentile of frame RMS (dB), a simple dynamic contrast measure.
    pub dynamic_contrast_db: f32,
    /// Detected onsets per second.
    pub rhythmic_density: f32,
    pub mean_centroid_hz: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioAnalysis {
    pub version: u32,
    pub sample_rate: u32,
    pub duration_samples: i64,
    pub channels: u16,
    pub tempo: TempoEstimate,
    pub downbeat_confidence: f32,
    pub beats: Vec<BeatEvent>,
    pub kicks: Vec<PercussionEvent>,
    pub snares: Vec<PercussionEvent>,
    pub sections: Vec<MusicalSection>,
    pub drops: Vec<DropCandidate>,
    pub silences: Vec<SilenceRegion>,
    pub curves: AnalysisCurves,
    pub summary: AudioSummary,
    pub warnings: Vec<String>,
    pub config: AnalysisConfig,
}

impl AudioAnalysis {
    pub fn duration_seconds(&self) -> f64 {
        self.duration_samples as f64 / self.sample_rate as f64
    }
    pub fn seconds(&self, sample: i64) -> f64 {
        sample as f64 / self.sample_rate as f64
    }
    pub fn main_drop(&self) -> Option<&DropCandidate> {
        self.drops.iter().find(|d| d.kind == DropKind::MainDrop)
    }
    pub fn section_at(&self, sample: i64) -> Option<&MusicalSection> {
        self.sections
            .iter()
            .find(|s| sample >= s.start_sample && sample < s.end_sample)
    }
    pub fn beat_period_samples(&self) -> f64 {
        60.0 * self.sample_rate as f64 / self.tempo.bpm.max(1.0)
    }
}
