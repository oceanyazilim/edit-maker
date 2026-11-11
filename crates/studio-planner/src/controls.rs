//! User-facing creative controls and variants. Every control is 0..1 with 0.5
//! meaning "use the preset as authored". Exact interpretation is implemented in
//! [`Effective::resolve`] and documented in docs/PLANNER.md.

use serde::{Deserialize, Serialize};

use crate::niche::NicheProfile;
use crate::presets::EditPreset;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CreativeControls {
    pub beat_sync_strength: f32,
    pub cut_density: f32,
    pub drop_impact: f32,
    pub effect_intensity: f32,
    pub scene_continuity: f32,
    pub motion_energy: f32,
    pub aggressiveness: f32,
    pub transition_frequency: f32,
}

impl Default for CreativeControls {
    fn default() -> Self {
        Self {
            beat_sync_strength: 0.5,
            cut_density: 0.5,
            drop_impact: 0.5,
            effect_intensity: 0.5,
            scene_continuity: 0.5,
            motion_energy: 0.5,
            aggressiveness: 0.5,
            transition_frequency: 0.5,
        }
    }
}

impl CreativeControls {
    pub fn clamped(&self) -> Self {
        let c = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.5
            }
        };
        Self {
            beat_sync_strength: c(self.beat_sync_strength),
            cut_density: c(self.cut_density),
            drop_impact: c(self.drop_impact),
            effect_intensity: c(self.effect_intensity),
            scene_continuity: c(self.scene_continuity),
            motion_energy: c(self.motion_energy),
            aggressiveness: c(self.aggressiveness),
            transition_frequency: c(self.transition_frequency),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Balanced,
    MaximumImpact,
    SmoothCinematic,
    HighMotion,
    MinimalClean,
    Experimental,
}

impl Variant {
    pub const ALL: [Variant; 6] = [
        Variant::Balanced,
        Variant::MaximumImpact,
        Variant::SmoothCinematic,
        Variant::HighMotion,
        Variant::MinimalClean,
        Variant::Experimental,
    ];
    pub fn code(self) -> &'static str {
        match self {
            Variant::Balanced => "bal",
            Variant::MaximumImpact => "max",
            Variant::SmoothCinematic => "cin",
            Variant::HighMotion => "mot",
            Variant::MinimalClean => "min",
            Variant::Experimental => "exp",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Variant::Balanced => "Balanced",
            Variant::MaximumImpact => "Maximum Impact",
            Variant::SmoothCinematic => "Smooth Cinematic",
            Variant::HighMotion => "High Motion",
            Variant::MinimalClean => "Minimal Clean",
            Variant::Experimental => "Experimental",
        }
    }
    /// Control adjustments applied on top of the user's controls.
    fn adjust(self, c: &mut CreativeControls) -> f32 {
        // returns selection temperature (randomness of choices)
        match self {
            Variant::Balanced => 0.08,
            Variant::MaximumImpact => {
                c.cut_density += 0.2;
                c.drop_impact += 0.3;
                c.effect_intensity += 0.2;
                c.aggressiveness += 0.25;
                c.motion_energy += 0.15;
                0.08
            }
            Variant::SmoothCinematic => {
                c.cut_density -= 0.3;
                c.transition_frequency += 0.35;
                c.effect_intensity -= 0.3;
                c.scene_continuity += 0.3;
                c.aggressiveness -= 0.3;
                0.08
            }
            Variant::HighMotion => {
                c.motion_energy += 0.4;
                c.cut_density += 0.1;
                0.1
            }
            Variant::MinimalClean => {
                c.effect_intensity = 0.05;
                c.transition_frequency = 0.1;
                c.cut_density -= 0.2;
                c.aggressiveness = 0.1;
                0.05
            }
            Variant::Experimental => {
                c.effect_intensity += 0.25;
                c.transition_frequency += 0.2;
                c.scene_continuity -= 0.3;
                0.35
            }
        }
    }
}

/// Fully resolved planning parameters.
#[derive(Clone, Debug)]
pub struct Effective {
    pub beat_sync: f32,
    /// Multiplier on target shot length (smaller = more cuts).
    pub shot_scale: f32,
    pub min_shot_secs: f32,
    pub max_shot_secs: f32,
    pub drop_impact: f32,
    pub effect_intensity: f32,
    pub kick_threshold: f32,
    pub continuity_weight: f32,
    pub energy_bias: f32,
    pub transition_prob: f32,
    pub temperature: f32,
}

impl Effective {
    pub fn resolve(
        p: &EditPreset,
        user: &CreativeControls,
        variant: Variant,
        niche: Option<&NicheProfile>,
    ) -> Effective {
        let mut c = user.clamped();
        if let Some(n) = niche {
            c.cut_density += n.cut_density_bias;
            c.effect_intensity += n.effect_intensity_bias;
            c.transition_frequency += n.transition_bias;
            c.motion_energy += n.energy_bias * 0.5;
        }
        let temperature = variant.adjust(&mut c);
        let c = c.clamped();
        let k = |v: f32| 2.0 * v; // 0.5 -> 1.0 (preset as authored)
        let aggr = c.aggressiveness;
        Effective {
            beat_sync: (p.beat_sync + (c.beat_sync_strength - 0.5)).clamp(0.0, 1.0),
            // cut_density 1.0 halves shot length, 0.0 doubles it.
            shot_scale: 2f32.powf((0.5 - c.cut_density) * 2.0),
            min_shot_secs: (p.pacing.min_shot_secs * (1.3 - 0.6 * aggr)).max(0.15),
            max_shot_secs: p.pacing.max_shot_secs,
            drop_impact: (p.drop.impact * k(c.drop_impact)).clamp(0.0, 1.0),
            effect_intensity: (p.effects.intensity * k(c.effect_intensity)).clamp(0.0, 1.0),
            // aggressive -> more kicks qualify
            kick_threshold: (p.effects.kick_threshold + (0.5 - aggr) * 0.4).clamp(0.05, 1.0),
            continuity_weight: p.scoring.continuity * k(c.scene_continuity),
            energy_bias: (p.energy_bias + (c.motion_energy - 0.5)).clamp(-1.0, 1.0),
            transition_prob: (p.transitions.frequency * k(c.transition_frequency)).clamp(0.0, 1.0),
            temperature,
        }
    }
}
