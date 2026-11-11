//! Shared effect specification. The same definitions drive preview and export
//! (one compositor implementation, see `studio-render`), which is how
//! preview/export parity is guaranteed.

use serde::{Deserialize, Serialize};
use studio_core::{StudioError, StudioResult, Tick};

/// Effect spec version; bump when parameter semantics change.
pub const EFFECT_SPEC_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectKind {
    /// Scale punch `1 + amount` at effect start, decaying exponentially.
    ZoomPunch { amount: f32 },
    /// Pseudo-random camera shake; `amplitude` as a fraction of canvas height.
    Shake {
        amplitude: f32,
        frequency_hz: f32,
        seed: u32,
    },
    /// White flash that decays over the effect duration.
    Flash { strength: f32 },
    /// Horizontal RGB channel split; `offset` as a fraction of canvas width, decaying.
    RgbSplit { offset: f32 },
    /// Static colour adjustment.
    Color {
        brightness: f32,
        contrast: f32,
        saturation: f32,
    },
    /// Darkened edges.
    Vignette { strength: f32 },
    /// Fade the clip from/to black over the effect duration (`direction`: in = from black).
    Fade { fade_in: bool },
    /// Digital glitch: horizontal slice displacement + channel separation, re-randomized
    /// ~15×/s, decaying over the effect. `strength` 0..1.
    Glitch { strength: f32, seed: u32 },
    /// Radial chromatic aberration (red scaled out, blue scaled in), decaying. `amount` = scale delta.
    ChromaticAberration { amount: f32 },
    /// Directional motion blur. `amount` = blur length as a fraction of canvas width; decays.
    MotionBlur { amount: f32, angle_deg: f32 },
    /// Radial zoom blur from the canvas centre. `amount` = radial streak length fraction; decays.
    ZoomBlur { amount: f32 },
}

impl EffectKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            EffectKind::ZoomPunch { .. } => "zoom_punch",
            EffectKind::Shake { .. } => "shake",
            EffectKind::Flash { .. } => "flash",
            EffectKind::RgbSplit { .. } => "rgb_split",
            EffectKind::Color { .. } => "color",
            EffectKind::Vignette { .. } => "vignette",
            EffectKind::Fade { .. } => "fade",
            EffectKind::Glitch { .. } => "glitch",
            EffectKind::ChromaticAberration { .. } => "chromatic_aberration",
            EffectKind::MotionBlur { .. } => "motion_blur",
            EffectKind::ZoomBlur { .. } => "zoom_blur",
        }
    }
    /// Validate parameters against their documented ranges.
    pub fn validate(&self) -> StudioResult<()> {
        let check = |name: &str, v: f32, lo: f32, hi: f32| {
            if !v.is_finite() || v < lo || v > hi {
                Err(StudioError::validation(format!(
                    "Effect parameter '{name}' must be within {lo}..{hi} (got {v})."
                )))
            } else {
                Ok(())
            }
        };
        match *self {
            EffectKind::ZoomPunch { amount } => check("amount", amount, 0.0, 1.0),
            EffectKind::Shake {
                amplitude,
                frequency_hz,
                ..
            } => {
                check("amplitude", amplitude, 0.0, 0.2)?;
                check("frequency_hz", frequency_hz, 0.1, 60.0)
            }
            EffectKind::Flash { strength } => check("strength", strength, 0.0, 1.0),
            EffectKind::RgbSplit { offset } => check("offset", offset, 0.0, 0.1),
            EffectKind::Color {
                brightness,
                contrast,
                saturation,
            } => {
                check("brightness", brightness, -1.0, 1.0)?;
                check("contrast", contrast, 0.0, 3.0)?;
                check("saturation", saturation, 0.0, 3.0)
            }
            EffectKind::Vignette { strength } => check("strength", strength, 0.0, 1.0),
            EffectKind::Fade { .. } => Ok(()),
            EffectKind::Glitch { strength, .. } => check("strength", strength, 0.0, 1.0),
            EffectKind::ChromaticAberration { amount } => check("amount", amount, 0.0, 0.1),
            EffectKind::MotionBlur { amount, angle_deg } => {
                check("amount", amount, 0.0, 0.2)?;
                check("angle_deg", angle_deg, -360.0, 360.0)
            }
            EffectKind::ZoomBlur { amount } => check("amount", amount, 0.0, 0.3),
        }
    }
}

/// An effect applied to a clip during `[clip.start + offset, … + duration)`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EffectInstance {
    pub id: String,
    pub kind: EffectKind,
    /// Offset from clip start.
    pub offset: Tick,
    pub duration: Tick,
    #[serde(default = "spec_v")]
    pub version: u32,
    /// Free-form provenance, e.g. "kick", "drop", "user".
    #[serde(default)]
    pub trigger: Option<String>,
}

fn spec_v() -> u32 {
    EFFECT_SPEC_VERSION
}

impl EffectInstance {
    /// Normalized progress (0..1) at clip-local time, or None if inactive.
    pub fn progress(&self, local: Tick) -> Option<f32> {
        if local < self.offset || local >= self.offset + self.duration || self.duration.0 <= 0 {
            return None;
        }
        Some((local - self.offset).0 as f32 / self.duration.0 as f32)
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    /// Brief white flash at the cut.
    FlashCut,
    /// Outgoing clip zooms in, incoming zooms out from a punch.
    ZoomBlend,
    /// Dip through black.
    DipToBlack,
}

/// Transition centred on the cut between the previous clip and this one.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration: Tick,
}

impl Transition {
    pub fn half(&self) -> Tick {
        Tick(self.duration.0 / 2)
    }
    /// Whether the transition needs frames beyond the clips' cut points.
    pub fn needs_handles(&self) -> bool {
        matches!(
            self.kind,
            TransitionKind::CrossDissolve | TransitionKind::ZoomBlend
        )
    }
}
