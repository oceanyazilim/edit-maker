use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use studio_core::{Rational, Tick};
use studio_media::{MediaKind, MediaMetadata};
use studio_planner::{CreativeControls, Cue, PlannedSegment, Variant};
use studio_timeline::{Timeline, TimelineSettings, Track, TrackKind};

pub const PROJECT_SCHEMA_VERSION: u32 = 1;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetRole {
    Music,
    /// Reusable source footage (user-confirmed rights). Only this role is used by the planner.
    Footage,
    Overlay,
    /// Trend / style reference: analyzed for creative structure only, never placed on the timeline.
    Reference,
}

/// User-declared usage rights. The application never verifies rights itself; it
/// records the user's confirmation and reminds them where relevant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct AssetRights {
    pub confirmed: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MediaAsset {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub fingerprint: String,
    pub kind: MediaKind,
    pub role: AssetRole,
    pub metadata: MediaMetadata,
    #[serde(default)]
    pub rights: AssetRights,
    #[serde(default)]
    pub thumbnail: Option<PathBuf>,
    #[serde(default)]
    pub proxy: Option<PathBuf>,
    #[serde(default)]
    pub missing: bool,
    pub imported_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProjectSettings {
    pub platform: String,
    pub aspect: String,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub duration_secs: f64,
    pub niche_id: Option<String>,
    pub preset_id: String,
    pub variant: Variant,
    pub seed: u64,
    pub controls: CreativeControls,
    /// User-pinned moments ("this clip moment at this second").
    #[serde(default)]
    pub cues: Vec<Cue>,
    /// Start of the music segment in song time (smart segment selector). None = automatic.
    #[serde(default)]
    pub audio_start_secs: Option<f64>,
    /// Desired main-drop position inside the output (0..1). None = preset default.
    #[serde(default)]
    pub drop_position: Option<f32>,
    /// Manual audio-analysis corrections; they persist and are re-applied after analysis.
    #[serde(default)]
    pub audio_edits: AudioEdits,
}

/// Persistent manual corrections to the automatic music analysis.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AudioEdits {
    /// Manual BPM (re-tracks beats at this tempo).
    pub bpm_override: Option<f64>,
    /// Shift of the whole beat grid in milliseconds.
    pub grid_offset_ms: f64,
    /// Drops added by the user (song seconds). The first becomes the main drop when
    /// the analysis has none.
    pub added_drops_secs: Vec<f64>,
    /// Detected drops removed by the user (matched within 0.3 s).
    pub removed_drops_secs: Vec<f64>,
}

/// A saved timeline state (generation history / manual snapshots).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimelineVersion {
    pub id: String,
    pub label: String,
    pub created_at: u64,
    pub seed: u64,
    pub timeline: Timeline,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            platform: "tiktok".into(),
            aspect: "9:16".into(),
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            duration_secs: 20.0,
            niche_id: None,
            preset_id: "high-energy-edm".into(),
            variant: Variant::Balanced,
            seed: 1,
            controls: CreativeControls::default(),
            cues: vec![],
            audio_start_secs: None,
            drop_position: None,
            audio_edits: AudioEdits::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PlanRecord {
    pub seed: u64,
    pub variant: Variant,
    pub preset_id: String,
    pub generated_at: u64,
    pub audio_start_sample: i64,
    pub audio_end_sample: i64,
    pub segments: Vec<PlannedSegment>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenderRecord {
    pub id: String,
    pub output: PathBuf,
    pub created_at: u64,
    pub encoder: String,
    pub width: u32,
    pub height: u32,
    pub duration: Tick,
    pub elapsed_secs: f64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    /// Owning profile. Files written before profiles existed load into the default profile.
    #[serde(default = "default_profile_id")]
    pub profile_id: String,
    pub name: String,
    pub created_at: u64,
    pub modified_at: u64,
    pub settings: ProjectSettings,
    pub assets: Vec<MediaAsset>,
    pub timeline: Timeline,
    #[serde(default)]
    pub plan: Option<PlanRecord>,
    #[serde(default)]
    pub renders: Vec<RenderRecord>,
    /// Previous timeline states, newest last (bounded).
    #[serde(default)]
    pub versions: Vec<TimelineVersion>,
    /// Path of the project file (not serialized).
    #[serde(skip)]
    pub file: Option<PathBuf>,
}

fn default_profile_id() -> String {
    crate::profile::DEFAULT_PROFILE_ID.to_string()
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve an aspect label to a default resolution (long side 1920).
pub fn resolution_for(aspect: &str) -> (u32, u32) {
    match aspect {
        "16:9" => (1920, 1080),
        "1:1" => (1080, 1080),
        "4:5" => (1080, 1350),
        _ => (1080, 1920),
    }
}

impl Project {
    pub fn new(name: impl Into<String>, settings: ProjectSettings) -> Project {
        let ts = now_secs();
        let tl = Project::empty_timeline(&settings);
        Project {
            schema_version: PROJECT_SCHEMA_VERSION,
            id: studio_core::ids::new_id("proj"),
            profile_id: default_profile_id(),
            name: name.into(),
            created_at: ts,
            modified_at: ts,
            settings,
            assets: vec![],
            timeline: tl,
            plan: None,
            renders: vec![],
            versions: vec![],
            file: None,
        }
    }
    pub fn empty_timeline(s: &ProjectSettings) -> Timeline {
        let mut tl = Timeline::new(TimelineSettings {
            width: s.width,
            height: s.height,
            fps: s.fps,
            sample_rate: 48000,
            background: [0, 0, 0, 255],
        });
        tl.tracks
            .push(Track::new("V1", TrackKind::Video, "Video 1"));
        tl.tracks.push(Track::new("A1", TrackKind::Audio, "Music"));
        tl
    }
    pub fn asset(&self, id: &str) -> Option<&MediaAsset> {
        self.assets.iter().find(|a| a.id == id)
    }
    pub fn music(&self) -> Option<&MediaAsset> {
        self.assets.iter().find(|a| a.role == AssetRole::Music)
    }
    pub fn footage(&self) -> impl Iterator<Item = &MediaAsset> {
        self.assets.iter().filter(|a| a.role == AssetRole::Footage)
    }
    pub fn touch(&mut self) {
        self.modified_at = now_secs();
    }
}
