//! Configurable niche profiles (no per-niche code paths). Built-ins are embedded
//! from `presets/niches/catalog.json`; users can add profiles with the same schema.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use studio_core::StudioResult;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NicheProfile {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    /// Weights for measurable scene tags (see studio-vision); positive = prefer.
    pub tag_weights: BTreeMap<String, f32>,
    pub default_preset: String,
    /// Offsets applied to creative controls (-0.5..0.5).
    pub cut_density_bias: f32,
    pub effect_intensity_bias: f32,
    pub transition_bias: f32,
    pub energy_bias: f32,
    pub recommended_aspects: Vec<String>,
    pub recommended_duration_secs: f32,
    pub suggested_music: Vec<String>,
    pub builtin: bool,
}

impl Default for NicheProfile {
    fn default() -> Self {
        Self {
            id: "general".into(),
            name: "General".into(),
            category: "General".into(),
            description: String::new(),
            tag_weights: BTreeMap::new(),
            default_preset: "fast-paced-montage".into(),
            cut_density_bias: 0.0,
            effect_intensity_bias: 0.0,
            transition_bias: 0.0,
            energy_bias: 0.0,
            recommended_aspects: vec!["9:16".into()],
            recommended_duration_secs: 20.0,
            suggested_music: vec![],
            builtin: false,
        }
    }
}

impl NicheProfile {
    /// Normalized tag preference in -1..1 for a scene's tags.
    pub fn tag_score(&self, tags: &[String]) -> f32 {
        if self.tag_weights.is_empty() {
            return 0.0;
        }
        let norm: f32 = self
            .tag_weights
            .values()
            .map(|v| v.abs())
            .sum::<f32>()
            .max(1e-6);
        let s: f32 = tags.iter().filter_map(|t| self.tag_weights.get(t)).sum();
        (s / norm * 2.0).clamp(-1.0, 1.0)
    }
    pub fn from_json(s: &str) -> StudioResult<NicheProfile> {
        Ok(serde_json::from_str(s)?)
    }
}

const CATALOG: &str = include_str!("../../../presets/niches/catalog.json");

pub fn builtin_niches() -> Vec<NicheProfile> {
    let mut v: Vec<NicheProfile> =
        serde_json::from_str(CATALOG).expect("niche catalog must be valid");
    for n in v.iter_mut() {
        n.builtin = true;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_loads_and_presets_exist() {
        let ns = builtin_niches();
        assert!(ns.len() >= 40, "catalog has {}", ns.len());
        let presets: Vec<String> = crate::presets::builtin_presets()
            .into_iter()
            .map(|p| p.id)
            .collect();
        for n in &ns {
            assert!(
                presets.contains(&n.default_preset),
                "{} -> unknown preset {}",
                n.id,
                n.default_preset
            );
        }
        let anime = ns.iter().find(|n| n.id == "anime").unwrap();
        assert!(anime.tag_score(&["high_motion".into(), "action_peak".into()]) > 0.0);
    }
}
