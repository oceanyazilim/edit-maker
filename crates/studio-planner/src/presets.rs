//! Data-driven editing presets. Built-in presets are embedded from
//! `presets/editing/*.json`; user presets use the same schema.

use serde::{Deserialize, Serialize};
use studio_core::{StudioError, StudioResult};

pub const PRESET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SectionMultipliers {
    pub intro: f32,
    pub groove: f32,
    pub build_up: f32,
    pub drop: f32,
    pub breakdown: f32,
    pub outro: f32,
}

impl Default for SectionMultipliers {
    fn default() -> Self {
        Self {
            intro: 2.0,
            groove: 1.0,
            build_up: 1.0,
            drop: 0.5,
            breakdown: 2.0,
            outro: 2.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pacing {
    /// Baseline shot length in beats (before section multipliers and cut density).
    pub base_shot_beats: f32,
    pub section_multipliers: SectionMultipliers,
    pub min_shot_secs: f32,
    pub max_shot_secs: f32,
    /// Gradually shorten shots through a build-up (1 = halve by its end).
    pub build_up_acceleration: f32,
}

impl Default for Pacing {
    fn default() -> Self {
        Self {
            base_shot_beats: 2.0,
            section_multipliers: SectionMultipliers::default(),
            min_shot_secs: 0.3,
            max_shot_secs: 4.0,
            build_up_acceleration: 0.5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DropSettings {
    /// 0..1 scaling of drop effects.
    pub impact: f32,
    /// Where the main drop should land in a trimmed output (fraction of duration).
    pub position: f32,
    pub flash: bool,
    pub zoom_punch: f32,
    pub rgb_split: bool,
    pub shake: bool,
    /// Avoid cuts in this many beats right before the drop (anticipation hold).
    pub pre_drop_hold_beats: f32,
    /// Use slow motion for the pre-drop shot when footage allows (speed factor, 1 = off).
    pub pre_drop_slowmo: f32,
    /// Velocity edit into the drop (0 = off). The pre-drop shot runs at 1+velocity speed,
    /// then drops to slow motion for its last 40 %, keeping source time continuous.
    pub velocity: f32,
    /// Glitch strength on the drop (0 = off).
    pub glitch: f32,
    /// Radial zoom-blur streak on the drop (0 = off).
    pub zoom_blur: f32,
}

impl Default for DropSettings {
    fn default() -> Self {
        Self {
            impact: 0.7,
            position: 0.35,
            flash: true,
            zoom_punch: 0.10,
            rgb_split: false,
            shake: false,
            pre_drop_hold_beats: 1.0,
            pre_drop_slowmo: 1.0,
            velocity: 0.0,
            glitch: 0.0,
            zoom_blur: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ColorGrade {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
}

impl Default for ColorGrade {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EffectSettings {
    pub intensity: f32,
    /// Zoom punch amount on strong kicks (before intensity scaling).
    pub kick_zoom: f32,
    /// Shake amplitude on kicks (fraction of height).
    pub kick_shake: f32,
    /// Minimum kick intensity that may trigger an effect.
    pub kick_threshold: f32,
    pub cooldown_beats: f32,
    pub max_per_bar: u32,
    /// Sections where kick effects are allowed.
    pub kick_sections: Vec<String>,
    pub color: ColorGrade,
    pub vignette: f32,
    /// Slow push-in on calm shots (zoom delta over the shot).
    pub slow_zoom: f32,
    /// Additional kick effect pool (0 = not used): glitch strength, motion-blur length,
    /// chromatic aberration amount.
    pub kick_glitch: f32,
    pub kick_blur: f32,
    pub kick_chroma: f32,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            intensity: 0.6,
            kick_zoom: 0.05,
            kick_shake: 0.0,
            kick_threshold: 0.5,
            cooldown_beats: 1.0,
            max_per_bar: 2,
            kick_sections: vec!["drop".into()],
            color: ColorGrade::default(),
            vignette: 0.0,
            slow_zoom: 0.04,
            kick_glitch: 0.0,
            kick_blur: 0.0,
            kick_chroma: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TransitionSettings {
    /// Base probability of a non-cut transition at an eligible boundary.
    pub frequency: f32,
    /// Allowed kinds (snake_case TransitionKind names).
    pub kinds: Vec<String>,
    pub duration_beats: f32,
    /// Sections where soft transitions are allowed.
    pub sections: Vec<String>,
}

impl Default for TransitionSettings {
    fn default() -> Self {
        Self {
            frequency: 0.2,
            kinds: vec!["cross_dissolve".into()],
            duration_beats: 1.0,
            sections: vec!["intro".into(), "breakdown".into(), "outro".into()],
        }
    }
}

/// Weights of the scene selection score (all ≥ 0; see docs/PLANNER.md).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ScoringWeights {
    pub energy_match: f32,
    pub quality: f32,
    pub action_alignment: f32,
    pub repetition_penalty: f32,
    pub continuity: f32,
    pub diversity: f32,
    pub niche_tags: f32,
    pub duration_fit: f32,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            energy_match: 1.0,
            quality: 0.5,
            action_alignment: 0.6,
            repetition_penalty: 1.5,
            continuity: 0.5,
            diversity: 0.6,
            niche_tags: 0.4,
            duration_fit: 0.3,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OutputRecommendation {
    pub platforms: Vec<String>,
    pub aspect: String,
    pub duration_secs: f32,
}

impl Default for OutputRecommendation {
    fn default() -> Self {
        Self {
            platforms: vec!["tiktok".into(), "reels".into(), "shorts".into()],
            aspect: "9:16".into(),
            duration_secs: 20.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditPreset {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: u32,
    pub description: String,
    pub builtin: bool,
    pub pacing: Pacing,
    /// 0..1 – how strongly cuts follow musical events (preset default for the control).
    pub beat_sync: f32,
    pub drop: DropSettings,
    pub effects: EffectSettings,
    pub transitions: TransitionSettings,
    pub scoring: ScoringWeights,
    /// Preferred visual energy offset (-1..1) relative to the music.
    pub energy_bias: f32,
    pub smart_crop: bool,
    pub fade_in_secs: f32,
    pub fade_out_secs: f32,
    pub output: OutputRecommendation,
    pub tags: Vec<String>,
}

impl Default for EditPreset {
    fn default() -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            id: "custom".into(),
            name: "Custom".into(),
            version: 1,
            description: String::new(),
            builtin: false,
            pacing: Pacing::default(),
            beat_sync: 0.8,
            drop: DropSettings::default(),
            effects: EffectSettings::default(),
            transitions: TransitionSettings::default(),
            scoring: ScoringWeights::default(),
            energy_bias: 0.0,
            smart_crop: true,
            fade_in_secs: 0.0,
            fade_out_secs: 0.6,
            output: OutputRecommendation::default(),
            tags: vec![],
        }
    }
}

impl EditPreset {
    pub fn from_json(s: &str) -> StudioResult<EditPreset> {
        let p: EditPreset = serde_json::from_str(s)?;
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> StudioResult<()> {
        let unit = |name: &str, v: f32| {
            if (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(StudioError::validation(format!(
                    "Preset '{}': '{name}' must be within 0..1.",
                    self.id
                )))
            }
        };
        if self.id.trim().is_empty() {
            return Err(StudioError::validation("Preset id must not be empty."));
        }
        unit("beat_sync", self.beat_sync)?;
        unit("drop.impact", self.drop.impact)?;
        unit("drop.position", self.drop.position)?;
        unit("effects.intensity", self.effects.intensity)?;
        unit("transitions.frequency", self.transitions.frequency)?;
        if self.pacing.min_shot_secs <= 0.05
            || self.pacing.max_shot_secs < self.pacing.min_shot_secs
            || self.pacing.base_shot_beats <= 0.0
        {
            return Err(StudioError::validation(format!(
                "Preset '{}': invalid pacing.",
                self.id
            )));
        }
        if !(0.25..=1.0).contains(&self.drop.pre_drop_slowmo) {
            return Err(StudioError::validation(format!(
                "Preset '{}': pre_drop_slowmo must be within 0.25..1.",
                self.id
            )));
        }
        Ok(())
    }
    pub fn section_multiplier(&self, kind: studio_audio::SectionKind) -> f32 {
        use studio_audio::SectionKind::*;
        let m = &self.pacing.section_multipliers;
        match kind {
            Intro => m.intro,
            Groove => m.groove,
            BuildUp => m.build_up,
            Drop => m.drop,
            Breakdown => m.breakdown,
            Outro => m.outro,
        }
    }
}

macro_rules! preset_files {
    ($($f:literal),* $(,)?) => {
        &[$(include_str!(concat!("../../../presets/editing/", $f))),*]
    };
}

const BUILTIN: &[&str] = preset_files!(
    "aggressive-phonk.json",
    "brazilian-funk-velocity.json",
    "anime-impact.json",
    "cinematic-dark.json",
    "smooth-aesthetic.json",
    "gaming-highlights.json",
    "fast-paced-montage.json",
    "dramatic-slow-motion.json",
    "commercial-product-promo.json",
    "music-release-teaser.json",
    "minimal-clean.json",
    "high-energy-edm.json",
    "character-showcase.json",
    "automotive-velocity.json",
    "atmospheric-ambient.json",
    "short-form-viral-hook.json",
);

pub fn builtin_presets() -> Vec<EditPreset> {
    BUILTIN
        .iter()
        .map(|s| {
            let mut p = EditPreset::from_json(s).expect("built-in preset must be valid");
            p.builtin = true;
            p
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_builtins_parse_and_are_distinct() {
        let ps = builtin_presets();
        assert_eq!(ps.len(), 16);
        let mut ids: Vec<_> = ps.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 16);
        // Presets must differ in behaviour, not only in name.
        for i in 0..ps.len() {
            for j in i + 1..ps.len() {
                let (a, b) = (&ps[i], &ps[j]);
                let same = a.pacing == b.pacing
                    && a.effects == b.effects
                    && a.drop == b.drop
                    && a.transitions == b.transitions;
                assert!(!same, "{} and {} are behaviourally identical", a.id, b.id);
            }
        }
    }
    #[test]
    fn rejects_invalid() {
        assert!(EditPreset::from_json(r#"{"id":"x","beat_sync":2.0}"#).is_err());
        assert!(EditPreset::from_json(r#"{"id":""}"#).is_err());
    }
}
