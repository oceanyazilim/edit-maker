//! Content profiles (channel / niche workspaces).
//!
//! A profile is a workspace, not a login. Ownership of projects, renders,
//! asset references, favourite presets and trend research is enforced in the
//! SQLite queries (`profile_id` foreign keys), not by frontend filtering.

use serde::{Deserialize, Serialize};
use studio_core::{Rational, StudioError, StudioResult};
use studio_planner::{CreativeControls, Variant};

use crate::model::{resolution_for, ProjectSettings};

pub const DEFAULT_PROFILE_ID: &str = "profile_default";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProfileDefaults {
    pub preset_id: String,
    pub variant: Variant,
    pub duration_secs: f64,
    pub aspect: String,
    pub fps: Rational,
    pub platform: String,
    pub controls: CreativeControls,
    pub export_encoder: String,
    pub export_quality: u8,
    /// Destination directory for exports (None = project folder/exports).
    pub export_dir: Option<String>,
    /// Default folder offered when importing media.
    pub media_folder: Option<String>,
    /// File naming convention: {project} {variant} {n} {date}
    pub naming: String,
}

impl Default for ProfileDefaults {
    fn default() -> Self {
        Self {
            preset_id: "high-energy-edm".into(),
            variant: Variant::Balanced,
            duration_secs: 20.0,
            aspect: "9:16".into(),
            fps: Rational::FPS_30,
            platform: "tiktok".into(),
            controls: CreativeControls::default(),
            export_encoder: "auto".into(),
            export_quality: 18,
            export_dir: None,
            media_folder: None,
            naming: "{project}-{variant}-{n}".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Emoji or 1–2 characters shown in the avatar.
    pub avatar: String,
    /// Avatar background colour (#rrggbb).
    pub color: String,
    pub description: String,
    pub primary_niche: Option<String>,
    pub niches: Vec<String>,
    pub platforms: Vec<String>,
    pub channel_urls: Vec<String>,
    pub branding: Option<String>,
    pub defaults: ProfileDefaults,
    pub archived: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: DEFAULT_PROFILE_ID.into(),
            name: "Default".into(),
            avatar: "D".into(),
            color: "#3a3a42".into(),
            description: String::new(),
            primary_niche: None,
            niches: vec![],
            platforms: vec!["tiktok".into(), "reels".into(), "shorts".into()],
            channel_urls: vec![],
            branding: None,
            defaults: ProfileDefaults::default(),
            archived: false,
            created_at: 0,
            updated_at: 0,
        }
    }
}

impl Profile {
    pub fn validate(&self) -> StudioResult<()> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(StudioError::validation(
                "Profile name must be 1–60 characters.",
            ));
        }
        if !(1.0..=600.0).contains(&self.defaults.duration_secs) {
            return Err(StudioError::validation(
                "Default duration must be between 1 and 600 seconds.",
            ));
        }
        if self.defaults.export_quality > 51 {
            return Err(StudioError::validation("Export quality must be 0–51."));
        }
        for u in &self.channel_urls {
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err(StudioError::validation(format!(
                    "Channel URL must start with http(s)://: {u}"
                )));
            }
        }
        if !self.color.starts_with('#') || self.color.len() != 7 {
            return Err(StudioError::validation("Avatar colour must be #rrggbb."));
        }
        Ok(())
    }

    /// Project settings initialised from this profile's defaults. Projects copy these
    /// values; later project edits never write back to the profile.
    pub fn project_settings(&self) -> ProjectSettings {
        let d = &self.defaults;
        let (w, h) = resolution_for(&d.aspect);
        ProjectSettings {
            platform: d.platform.clone(),
            aspect: d.aspect.clone(),
            width: w,
            height: h,
            fps: d.fps,
            duration_secs: d.duration_secs,
            niche_id: self.primary_niche.clone(),
            preset_id: d.preset_id.clone(),
            variant: d.variant,
            seed: 1,
            controls: d.controls.clone(),
            cues: vec![],
            audio_start_secs: None,
            drop_position: None,
            audio_edits: Default::default(),
        }
    }
}
