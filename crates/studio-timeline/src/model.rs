use serde::{Deserialize, Serialize};
use studio_core::{Rational, Tick};

use crate::effects::{EffectInstance, Transition};

pub const TIMELINE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimelineSettings {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub sample_rate: u32,
    /// RGBA background shown where no video clip is active.
    pub background: [u8; 4],
}

impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            fps: Rational::FPS_30,
            sample_rate: 48000,
            background: [0, 0, 0, 255],
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    pub name: String,
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub hidden: bool,
}

impl Track {
    pub fn new(id: impl Into<String>, kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            name: name.into(),
            clips: vec![],
            locked: false,
            muted: false,
            hidden: false,
        }
    }
    pub fn sort(&mut self) {
        self.clips.sort_by_key(|c| c.start);
    }
    pub fn end(&self) -> Tick {
        self.clips
            .iter()
            .map(|c| c.end())
            .max()
            .unwrap_or(Tick::ZERO)
    }
    /// Clip active at `t` (tracks are non-overlapping).
    pub fn clip_at(&self, t: Tick) -> Option<(usize, &Clip)> {
        self.clips
            .iter()
            .enumerate()
            .find(|(_, c)| t >= c.start && t < c.end())
    }
}

/// How a source frame is placed into the output canvas.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    /// Scale to cover the canvas (crop overflow). Default for social formats.
    Cover,
    /// Scale to fit inside the canvas (letterbox/pillarbox).
    Fit,
}

/// Framing of the source inside the canvas. `center_*` are normalized source
/// coordinates (0..1) of the point placed at the canvas centre; `zoom` multiplies
/// the fit scale. Smart crop writes these values.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Framing {
    pub mode: FitMode,
    pub center_x: f32,
    pub center_y: f32,
    pub zoom: f32,
    pub rotation_deg: f32,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            mode: FitMode::Cover,
            center_x: 0.5,
            center_y: 0.5,
            zoom: 1.0,
            rotation_deg: 0.0,
        }
    }
}

/// Keyframed transform animation relative to the clip start. Values multiply / offset
/// the static [`Framing`]. Interpolation is linear between keys, held outside.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransformKeyframe {
    /// Offset from clip start.
    pub t: Tick,
    pub zoom: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    #[serde(default)]
    pub ease: Ease,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    EaseInOut,
    EaseOut,
    Hold,
}

impl Ease {
    pub fn apply(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Ease::Linear => x,
            Ease::EaseInOut => x * x * (3.0 - 2.0 * x),
            Ease::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
            Ease::Hold => 0.0,
        }
    }
}

/// Why the planner created a clip (automation data, shown in the inspector).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ClipOrigin {
    pub section: Option<String>,
    pub reason: Option<String>,
    pub scene_id: Option<String>,
    pub score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Clip {
    pub id: String,
    pub asset_id: String,
    pub start: Tick,
    pub duration: Tick,
    pub source_in: Tick,
    /// Source seconds per timeline second (constant speed; 1.0 = normal).
    pub speed: f64,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default)]
    pub framing: Framing,
    #[serde(default)]
    pub keyframes: Vec<TransformKeyframe>,
    #[serde(default)]
    pub effects: Vec<EffectInstance>,
    /// Transition from the previous clip on the same track into this one.
    #[serde(default)]
    pub transition_in: Option<Transition>,
    #[serde(default)]
    pub fade_in: Tick,
    #[serde(default)]
    pub fade_out: Tick,
    #[serde(default)]
    pub origin: ClipOrigin,
    /// Text clips: literal content.
    #[serde(default)]
    pub text: Option<String>,
    /// Protected from regeneration and from timing edits (move/trim/split/delete).
    /// Parameters and effects stay editable in the inspector.
    #[serde(default)]
    pub locked: bool,
}

fn one() -> f32 {
    1.0
}

impl Clip {
    pub fn new(
        id: impl Into<String>,
        asset_id: impl Into<String>,
        start: Tick,
        duration: Tick,
        source_in: Tick,
    ) -> Self {
        Self {
            id: id.into(),
            asset_id: asset_id.into(),
            start,
            duration,
            source_in,
            speed: 1.0,
            volume: 1.0,
            framing: Framing::default(),
            keyframes: vec![],
            effects: vec![],
            transition_in: None,
            fade_in: Tick::ZERO,
            fade_out: Tick::ZERO,
            origin: ClipOrigin::default(),
            text: None,
            locked: false,
        }
    }
    pub fn end(&self) -> Tick {
        self.start + self.duration
    }
    /// Source time shown at timeline time `t` (may be outside the clip for handles).
    pub fn source_time(&self, t: Tick) -> Tick {
        self.source_in + (t - self.start).scale_f64(self.speed)
    }
    pub fn source_out(&self) -> Tick {
        self.source_time(self.end())
    }
    /// Interpolated (zoom, offset_x, offset_y) from keyframes at offset `local` from clip start.
    pub fn transform_at(&self, local: Tick) -> (f32, f32, f32) {
        let k = &self.keyframes;
        if k.is_empty() {
            return (1.0, 0.0, 0.0);
        }
        if local <= k[0].t {
            return (k[0].zoom, k[0].offset_x, k[0].offset_y);
        }
        for w in k.windows(2) {
            if local >= w[0].t && local < w[1].t {
                let span = (w[1].t - w[0].t).0.max(1) as f32;
                let x = w[1].ease.apply((local - w[0].t).0 as f32 / span);
                let lerp = |a: f32, b: f32| a + (b - a) * x;
                return (
                    lerp(w[0].zoom, w[1].zoom),
                    lerp(w[0].offset_x, w[1].offset_x),
                    lerp(w[0].offset_y, w[1].offset_y),
                );
            }
        }
        let l = k.last().unwrap();
        (l.zoom, l.offset_x, l.offset_y)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    pub id: String,
    pub t: Tick,
    pub label: String,
    #[serde(default)]
    pub kind: MarkerKind,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    #[default]
    User,
    Beat,
    Downbeat,
    Kick,
    Drop,
    Section,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Timeline {
    pub schema_version: u32,
    pub settings: TimelineSettings,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// Explicit output duration; defaults to the end of the last clip.
    #[serde(default)]
    pub duration: Option<Tick>,
}

impl Timeline {
    pub fn new(settings: TimelineSettings) -> Self {
        Self {
            schema_version: TIMELINE_SCHEMA_VERSION,
            settings,
            tracks: vec![],
            markers: vec![],
            duration: None,
        }
    }
    pub fn duration(&self) -> Tick {
        self.duration.unwrap_or_else(|| {
            self.tracks
                .iter()
                .map(|t| t.end())
                .max()
                .unwrap_or(Tick::ZERO)
        })
    }
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn track_mut(&mut self, id: &str) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }
    pub fn find_clip(&self, clip_id: &str) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ci) = t.clips.iter().position(|c| c.id == clip_id) {
                return Some((ti, ci));
            }
        }
        None
    }
    pub fn clip(&self, clip_id: &str) -> Option<&Clip> {
        self.find_clip(clip_id)
            .map(|(t, c)| &self.tracks[t].clips[c])
    }
    pub fn video_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(|t| t.kind == TrackKind::Video)
    }
    pub fn audio_tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(|t| t.kind == TrackKind::Audio)
    }
    /// All clip boundaries + markers, for snapping.
    pub fn snap_points(&self) -> Vec<Tick> {
        let mut v: Vec<Tick> = vec![Tick::ZERO];
        for t in &self.tracks {
            for c in &t.clips {
                v.push(c.start);
                v.push(c.end());
            }
        }
        v.extend(self.markers.iter().map(|m| m.t));
        v.sort();
        v.dedup();
        v
    }
    /// Snap `t` to the nearest snap point within `tolerance`.
    pub fn snap(&self, t: Tick, tolerance: Tick) -> Tick {
        self.snap_points()
            .into_iter()
            .map(|p| (p, (p - t).abs()))
            .filter(|(_, d)| *d <= tolerance)
            .min_by_key(|(_, d)| *d)
            .map(|(p, _)| p)
            .unwrap_or(t)
    }
}
