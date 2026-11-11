//! Automation Engine: import → analysis (cached) → planning → preview → render.
//! Used by both the desktop app and the CLI so they share one implementation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{de::DeserializeOwned, Serialize};
use studio_audio::{AnalysisConfig, AudioAnalysis, DropCandidate, DropFeatures, DropKind};
use studio_core::cancel::{noop_progress, report, sub_progress, ProgressSink};
use studio_core::{CancelToken, ErrorCode, StudioError, StudioResult, Tick};
use studio_media::{EncoderInfo, FfmpegTools, MediaKind, MediaMetadata};
use studio_planner::{
    builtin_niches, builtin_presets, EditPlan, EditPreset, FootageSource, NicheProfile,
    OutputTarget, PlanInput,
};
use studio_render::{RenderAsset, RenderAssetKind, RenderReport, RenderSettings};
use studio_timeline::{Clip, TrackKind};
use studio_vision::{VideoAnalysis, VisionConfig};

use crate::library::{Library, RenderHistoryEntry};
use crate::model::*;
use crate::store;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub projects: PathBuf,
}

impl AppDirs {
    /// Windows defaults: %LOCALAPPDATA%\AIAutoEditStudio and Documents\AI Auto Edit Studio.
    pub fn default_for_user() -> AppDirs {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let docs = std::env::var_os("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Documents"))
            .unwrap_or_else(|| local.clone());
        let data = local.join("AIAutoEditStudio");
        AppDirs {
            cache: data.join("cache"),
            data,
            projects: docs.join("AI Auto Edit Studio"),
        }
    }
    pub fn under(root: &Path) -> AppDirs {
        AppDirs {
            data: root.join("data"),
            cache: root.join("cache"),
            projects: root.join("projects"),
        }
    }
}

pub struct Engine {
    pub dirs: AppDirs,
    pub library: Library,
    tools: RwLock<Option<FfmpegTools>>,
    tool_dirs: RwLock<Vec<PathBuf>>,
    encoders: RwLock<Option<Vec<EncoderInfo>>>,
}

fn short_hash(v: &impl Serialize) -> String {
    let s = serde_json::to_vec(v).unwrap_or_default();
    blake3::hash(&s).to_hex()[..10].to_string()
}

impl Engine {
    pub fn new(dirs: AppDirs) -> StudioResult<Engine> {
        for d in [&dirs.data, &dirs.cache, &dirs.projects] {
            std::fs::create_dir_all(d)?;
        }
        let library = Library::open(&dirs.data.join("library.db"))?;
        Ok(Engine {
            dirs,
            library,
            tools: RwLock::new(None),
            tool_dirs: RwLock::new(vec![]),
            encoders: RwLock::new(None),
        })
    }

    // ---------- tools ----------

    pub fn tools(&self) -> StudioResult<FfmpegTools> {
        if let Some(t) = self.tools.read().unwrap().clone() {
            return Ok(t);
        }
        let t = FfmpegTools::locate(&self.tool_dirs.read().unwrap())?;
        *self.tools.write().unwrap() = Some(t.clone());
        Ok(t)
    }

    pub fn set_ffmpeg_dir(&self, dir: &Path) -> StudioResult<FfmpegTools> {
        let t = FfmpegTools::from_dir(dir)?;
        *self.tool_dirs.write().unwrap() = vec![dir.to_path_buf()];
        *self.tools.write().unwrap() = Some(t.clone());
        *self.encoders.write().unwrap() = None;
        Ok(t)
    }

    pub fn encoders(&self) -> StudioResult<Vec<EncoderInfo>> {
        if let Some(e) = self.encoders.read().unwrap().clone() {
            return Ok(e);
        }
        let e = studio_media::detect_encoders(&self.tools()?)?;
        *self.encoders.write().unwrap() = Some(e.clone());
        Ok(e)
    }

    // ---------- presets / niches ----------

    pub fn user_presets_dir(&self) -> PathBuf {
        self.dirs.data.join("presets")
    }

    pub fn presets(&self) -> Vec<EditPreset> {
        let mut v = builtin_presets();
        if let Ok(rd) = std::fs::read_dir(self.user_presets_dir()) {
            for e in rd.flatten() {
                if e.path().extension().map(|x| x == "json").unwrap_or(false) {
                    match std::fs::read_to_string(e.path())
                        .map_err(StudioError::from)
                        .and_then(|s| EditPreset::from_json(&s))
                    {
                        Ok(p) if !v.iter().any(|b| b.id == p.id) => v.push(p),
                        Ok(_) => tracing::warn!(
                            "user preset id collides with a built-in: {}",
                            e.path().display()
                        ),
                        Err(err) => tracing::warn!(
                            "invalid user preset {}: {}",
                            e.path().display(),
                            err.message
                        ),
                    }
                }
            }
        }
        v
    }

    pub fn save_user_preset(&self, p: &EditPreset) -> StudioResult<PathBuf> {
        p.validate()?;
        if builtin_presets().iter().any(|b| b.id == p.id) {
            return Err(StudioError::validation(
                "Built-in presets cannot be overwritten; duplicate it with a new id.",
            ));
        }
        let name = studio_core::ids::sanitize_file_stem(&p.id);
        let path = self.user_presets_dir().join(format!("{name}.json"));
        let mut q = p.clone();
        q.builtin = false;
        store::atomic_write(&path, &serde_json::to_vec_pretty(&q)?)?;
        Ok(path)
    }

    pub fn niches(&self) -> Vec<NicheProfile> {
        builtin_niches()
    }

    // ---------- projects ----------

    // ---------- profiles ----------

    pub fn active_profile(&self) -> StudioResult<crate::profile::Profile> {
        self.library.active_profile()
    }

    /// Project settings for a new project in the active profile (profile defaults).
    pub fn default_project_settings(&self) -> StudioResult<ProjectSettings> {
        Ok(self.active_profile()?.project_settings())
    }

    /// Ensure a project belongs to the active profile (backend-enforced isolation).
    pub fn check_owner(&self, project: &Project) -> StudioResult<()> {
        let active = self.library.active_profile_id()?;
        let owner = self
            .library
            .project_owner(&project.id)?
            .unwrap_or_else(|| project.profile_id.clone());
        if owner != active || project.profile_id != active {
            let name = self
                .library
                .get_profile(&owner)?
                .map(|p| p.name)
                .unwrap_or(owner);
            return Err(StudioError::new(ErrorCode::Validation, format!("This project belongs to the profile \"{name}\". Switch to that profile to open it.")));
        }
        Ok(())
    }

    /// Create a project in the active profile.
    pub fn create_project(&self, name: &str, settings: ProjectSettings) -> StudioResult<Project> {
        let profile = self.active_profile()?;
        let mut p = Project::new(name, settings);
        p.profile_id = profile.id.clone();
        let stem = studio_core::ids::sanitize_file_stem(name);
        let folder = self
            .dirs
            .projects
            .join(format!("{stem}-{}", &p.id[p.id.len() - 6..]));
        p.file = Some(folder.join(format!("{stem}.{}", store::PROJECT_EXT)));
        self.save_project(&mut p)?;
        Ok(p)
    }

    pub fn save_project(&self, p: &mut Project) -> StudioResult<PathBuf> {
        self.check_owner(p)?;
        let f = store::save(p)?;
        self.library
            .upsert_project(&p.profile_id, &p.id, &p.name, &f, p.modified_at)?;
        Ok(f)
    }

    pub fn open_project(&self, path: &Path) -> StudioResult<store::LoadResult> {
        let r = store::load(path)?;
        self.check_owner(&r.project)?;
        self.library.upsert_project(
            &r.project.profile_id,
            &r.project.id,
            &r.project.name,
            path,
            r.project.modified_at,
        )?;
        Ok(r)
    }

    pub fn project_dir(&self, p: &Project) -> PathBuf {
        p.file
            .as_ref()
            .and_then(|f| f.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| self.dirs.projects.clone())
    }

    // ---------- import ----------

    fn audio_metadata_native(path: &Path, cancel: &CancelToken) -> StudioResult<MediaMetadata> {
        let d = studio_audio::decode_file(path, cancel)?;
        Ok(MediaMetadata {
            kind: MediaKind::Audio,
            container: path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default(),
            duration: Tick::from_samples(d.samples.len() as i64, d.sample_rate),
            size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            bit_rate: None,
            video: None,
            audio: vec![studio_media::AudioStreamInfo {
                index: 0,
                codec: d.codec,
                sample_rate: d.sample_rate,
                channels: d.channels as u32,
                channel_layout: None,
                bit_rate: None,
            }],
            warnings: if d.skipped_packets > 0 {
                vec![format!(
                    "{} corrupt audio packets were skipped.",
                    d.skipped_packets
                )]
            } else {
                vec![]
            },
        })
    }

    pub fn import_media(
        &self,
        project: &mut Project,
        path: &Path,
        role: AssetRole,
        cancel: &CancelToken,
    ) -> StudioResult<MediaAsset> {
        let path = studio_media::validate_input_path(path)?;
        let fingerprint = studio_media::fingerprint(&path)?;
        if let Some(existing) = project
            .assets
            .iter()
            .find(|a| a.fingerprint == fingerprint && a.role == role)
        {
            return Ok(existing.clone());
        }
        let tools = self.tools();
        let metadata = match &tools {
            Ok(t) => studio_media::probe(t, &path)?,
            Err(e) if role == AssetRole::Music => {
                tracing::info!("FFmpeg unavailable ({}), probing audio natively", e.message);
                Self::audio_metadata_native(&path, cancel)?
            }
            Err(e) => return Err(e.clone()),
        };
        match role {
            AssetRole::Music if !metadata.has_audio() => {
                return Err(StudioError::invalid_media(
                    "The selected music file has no audio stream.",
                ))
            }
            AssetRole::Footage | AssetRole::Reference if !metadata.has_video() => {
                return Err(StudioError::invalid_media(
                    "The selected footage has no video stream.",
                ))
            }
            _ => {}
        }
        let mut thumbnail = None;
        if let (Ok(t), Some(_)) = (&tools, &metadata.video) {
            let thumb = self
                .dirs
                .cache
                .join("thumbs")
                .join(format!("{fingerprint}.jpg"));
            if !thumb.exists() {
                std::fs::create_dir_all(thumb.parent().unwrap())?;
                let at = if metadata.kind == MediaKind::Image {
                    Tick::ZERO
                } else {
                    Tick::from_seconds_f64((metadata.duration.as_seconds_f64() / 3.0).min(1.0))
                };
                if let Err(e) =
                    studio_media::transcode::thumbnail(t, &path, at, 320, &thumb, cancel)
                {
                    tracing::warn!("thumbnail failed: {}", e.message);
                }
            }
            if thumb.exists() {
                thumbnail = Some(thumb);
            }
        }
        let kind_s = format!("{:?}", metadata.kind).to_lowercase();
        if let Some(prev) = self.library.note_media(
            &project.profile_id,
            &fingerprint,
            &path,
            &kind_s,
            metadata.size_bytes,
            metadata.duration.as_seconds_f64(),
            now_secs(),
        )? {
            tracing::info!(
                "duplicate media: {} was previously seen at {}",
                path.display(),
                prev.display()
            );
        }
        let asset = MediaAsset {
            id: studio_core::ids::new_id("asset"),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            path,
            fingerprint,
            kind: metadata.kind,
            role,
            metadata,
            rights: AssetRights::default(),
            thumbnail,
            proxy: None,
            missing: false,
            imported_at: now_secs(),
        };
        if role == AssetRole::Music {
            // One music track per project in this version.
            project.assets.retain(|a| a.role != AssetRole::Music);
        }
        project.assets.push(asset.clone());
        project.touch();
        Ok(asset)
    }

    /// Import every video in `dir` (non-recursive) as footage. Files already in the project
    /// (same content fingerprint) are skipped. Returns (imported, skipped, errors).
    pub fn import_folder(
        &self,
        project: &mut Project,
        dir: &Path,
        cancel: &CancelToken,
    ) -> StudioResult<(Vec<MediaAsset>, usize, Vec<String>)> {
        const EXT: &[&str] = &["mp4", "mov", "mkv", "webm", "m4v", "avi"];
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| {
                StudioError::not_found("The folder could not be opened.")
                    .with_details(e.to_string())
            })?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|x| EXT.contains(&x.to_string_lossy().to_ascii_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .collect();
        files.sort();
        let (mut imported, mut skipped, mut errors) = (vec![], 0usize, vec![]);
        for f in files {
            cancel.check()?;
            let before = project.assets.len();
            match self.import_media(project, &f, AssetRole::Footage, cancel) {
                Ok(a) if project.assets.len() > before => imported.push(a),
                Ok(_) => skipped += 1,
                Err(e) => errors.push(format!("{}: {}", f.display(), e.message)),
            }
        }
        Ok((imported, skipped, errors))
    }

    // ---------- cache ----------

    fn cache_path(&self, key: &str) -> PathBuf {
        self.dirs.cache.join("analysis").join(format!("{key}.json"))
    }

    fn cached<T: DeserializeOwned>(&self, key: &str, version: u32) -> Option<T> {
        let p = self.library.cache_get(key, version).ok()??;
        serde_json::from_slice(&std::fs::read(p).ok()?).ok()
    }

    fn store_cache<T: Serialize>(
        &self,
        key: &str,
        kind: &str,
        version: u32,
        v: &T,
    ) -> StudioResult<()> {
        let p = self.cache_path(key);
        store::atomic_write(&p, &serde_json::to_vec(v)?)?;
        self.library.cache_put(key, kind, version, &p, now_secs())
    }

    // ---------- analysis ----------

    fn decode_music(
        &self,
        asset: &MediaAsset,
        cancel: &CancelToken,
    ) -> StudioResult<studio_audio::DecodedAudio> {
        match studio_audio::decode_file(&asset.path, cancel) {
            Ok(d) => Ok(d),
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::UnsupportedFormat | ErrorCode::InvalidMedia
                ) =>
            {
                // Fallback: let FFmpeg decode to WAV (e.g. Opus, AC-3, audio inside video containers).
                let tools = self.tools().map_err(|_| e.clone())?;
                let wav = self
                    .dirs
                    .cache
                    .join("audio")
                    .join(format!("{}.wav", asset.fingerprint));
                if !wav.exists() {
                    std::fs::create_dir_all(wav.parent().unwrap())?;
                    studio_media::transcode::extract_audio_wav(&tools, &asset.path, &wav, cancel)?;
                }
                studio_audio::decode_file(&wav, cancel)
            }
            Err(e) => Err(e),
        }
    }

    pub fn audio_analysis(
        &self,
        asset: &MediaAsset,
        cfg: &AnalysisConfig,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<AudioAnalysis> {
        let key = format!(
            "audio-{}-v{}-{}",
            asset.fingerprint,
            studio_audio::ANALYSIS_VERSION,
            short_hash(cfg)
        );
        if let Some(a) = self.cached::<AudioAnalysis>(&key, studio_audio::ANALYSIS_VERSION) {
            report(progress, 1.0, "Audio analysis (cached)");
            return Ok(a);
        }
        report(progress, 0.0, "Decoding audio");
        let audio = self.decode_music(asset, cancel)?;
        let a = studio_audio::analyze(&audio, cfg, cancel, &sub_progress(progress, 0.15, 1.0))?;
        self.store_cache(&key, "audio", studio_audio::ANALYSIS_VERSION, &a)?;
        // Waveform is cheap once decoded; cache it too.
        let wkey = format!("wave-{}-v1", asset.fingerprint);
        if self
            .cached::<studio_audio::waveform::WaveformPeaks>(&wkey, 1)
            .is_none()
        {
            let w = studio_audio::waveform::compute_peaks(&audio.samples, audio.sample_rate, 200);
            self.store_cache(&wkey, "waveform", 1, &w)?;
        }
        Ok(a)
    }

    /// Cached analyses only (never computes) – for UI reads.
    pub fn cached_audio_analysis(&self, asset: &MediaAsset) -> Option<AudioAnalysis> {
        let key = format!(
            "audio-{}-v{}-{}",
            asset.fingerprint,
            studio_audio::ANALYSIS_VERSION,
            short_hash(&AnalysisConfig::default())
        );
        self.cached(&key, studio_audio::ANALYSIS_VERSION)
    }

    pub fn cached_audio_analysis_cfg(
        &self,
        asset: &MediaAsset,
        cfg: &AnalysisConfig,
    ) -> Option<AudioAnalysis> {
        let key = format!(
            "audio-{}-v{}-{}",
            asset.fingerprint,
            studio_audio::ANALYSIS_VERSION,
            short_hash(cfg)
        );
        self.cached(&key, studio_audio::ANALYSIS_VERSION)
    }

    pub fn cached_video_analysis(&self, asset: &MediaAsset) -> Option<VideoAnalysis> {
        self.cached_video_analysis_fp(&asset.fingerprint)
    }

    pub fn cached_video_analysis_fp(&self, fingerprint: &str) -> Option<VideoAnalysis> {
        let key = format!(
            "vision-{}-v{}-{}",
            fingerprint,
            studio_vision::VISION_VERSION,
            short_hash(&VisionConfig::default())
        );
        self.cached(&key, studio_vision::VISION_VERSION)
    }

    /// Clip Discovery over the active profile's indexed library (own + shared media).
    /// Uses cached scene analyses only – files are never rescanned per query.
    pub fn discover_clips(
        &self,
        q: &crate::discovery::ClipQuery,
    ) -> StudioResult<crate::discovery::DiscoveryResult> {
        let profile = self.library.active_profile_id()?;
        let assets = self.library.profile_assets(&profile)?;
        Ok(crate::discovery::search(
            q,
            assets
                .iter()
                .filter(|a| a.kind == "video")
                .map(|a| (a, self.cached_video_analysis_fp(&a.fingerprint))),
        ))
    }

    pub fn waveform(
        &self,
        asset: &MediaAsset,
        cancel: &CancelToken,
    ) -> StudioResult<studio_audio::waveform::WaveformPeaks> {
        let key = format!("wave-{}-v1", asset.fingerprint);
        if let Some(w) = self.cached(&key, 1) {
            return Ok(w);
        }
        let audio = self.decode_music(asset, cancel)?;
        let w = studio_audio::waveform::compute_peaks(&audio.samples, audio.sample_rate, 200);
        self.store_cache(&key, "waveform", 1, &w)?;
        Ok(w)
    }

    pub fn video_analysis(
        &self,
        asset: &MediaAsset,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<VideoAnalysis> {
        let cfg = VisionConfig::default();
        let key = format!(
            "vision-{}-v{}-{}",
            asset.fingerprint,
            studio_vision::VISION_VERSION,
            short_hash(&cfg)
        );
        if let Some(a) = self.cached::<VideoAnalysis>(&key, studio_vision::VISION_VERSION) {
            report(progress, 1.0, "Scene analysis (cached)");
            return Ok(a);
        }
        let tools = self.tools()?;
        let a = studio_media::analyze::analyze_video(
            &tools,
            &asset.path,
            &asset.metadata,
            &cfg,
            cancel,
            progress,
        )?;
        self.store_cache(&key, "vision", studio_vision::VISION_VERSION, &a)?;
        Ok(a)
    }

    // ---------- generation ----------

    /// Music analysis with the project's manual corrections applied.
    pub fn corrected_audio_analysis(
        &self,
        music: &MediaAsset,
        edits: &AudioEdits,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<AudioAnalysis> {
        let cfg = AnalysisConfig {
            bpm_override: edits.bpm_override,
            ..Default::default()
        };
        let mut a = self.audio_analysis(music, &cfg, cancel, progress)?;
        apply_audio_edits(&mut a, edits);
        Ok(a)
    }

    pub fn suggest_segments(
        &self,
        project: &Project,
        duration_secs: f64,
    ) -> StudioResult<Vec<studio_planner::SegmentSuggestion>> {
        let music = project
            .music()
            .cloned()
            .ok_or_else(|| StudioError::invalid_input("Import a music track first."))?;
        let a = self.corrected_audio_analysis(
            &music,
            &project.settings.audio_edits,
            &CancelToken::new(),
            &noop_progress(),
        )?;
        Ok(studio_planner::recommend_segments(&a, duration_secs))
    }

    /// Generate a fresh edit (no locks).
    pub fn generate(
        &self,
        project: &mut Project,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<EditPlan> {
        self.regenerate(project, &RegenMode::All, cancel, progress)
    }

    /// Restore a saved timeline version (the current state is saved first, so it is undoable).
    pub fn restore_version(&self, project: &mut Project, version_id: &str) -> StudioResult<()> {
        let v = project
            .versions
            .iter()
            .find(|v| v.id == version_id)
            .cloned()
            .ok_or_else(|| StudioError::not_found("Version not found."))?;
        push_version(project, "Before restore");
        project.timeline = v.timeline;
        project.touch();
        Ok(())
    }

    /// Generate or partially regenerate the edit. Every call saves the previous timeline
    /// as a version first. Locked clips are preserved exactly; modes add temporary locks.
    pub fn regenerate(
        &self,
        project: &mut Project,
        mode: &RegenMode,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<EditPlan> {
        let music = project.music().cloned().ok_or_else(|| {
            StudioError::invalid_input("Import a music track before generating an edit.")
        })?;
        let footage: Vec<MediaAsset> = project.footage().filter(|a| !a.missing).cloned().collect();
        if footage.is_empty() {
            return Err(StudioError::invalid_input(
                "Import at least one video before generating an edit.",
            ));
        }
        if music.missing {
            return Err(StudioError::not_found("Source media could not be found.")
                .with_details(music.path.display().to_string()));
        }
        let audio = self.corrected_audio_analysis(
            &music,
            &project.settings.audio_edits,
            cancel,
            &sub_progress(progress, 0.0, 0.35),
        )?;
        let mut analyses = Vec::new();
        let n = footage.len() as f32;
        for (i, f) in footage.iter().enumerate() {
            cancel.check()?;
            let lo = 0.35 + 0.5 * i as f32 / n;
            analyses.push(self.video_analysis(
                f,
                cancel,
                &sub_progress(progress, lo, lo + 0.5 / n),
            )?);
        }
        report(progress, 0.9, "Planning edit");
        let presets = self.presets();
        let preset = presets
            .iter()
            .find(|p| p.id == project.settings.preset_id)
            .ok_or_else(|| {
                StudioError::not_found(format!(
                    "Preset '{}' was not found.",
                    project.settings.preset_id
                ))
            })?;
        let niches = self.niches();
        let niche = project
            .settings
            .niche_id
            .as_ref()
            .and_then(|id| niches.iter().find(|n| &n.id == id));
        let s = project.settings.clone();
        let sources: Vec<FootageSource> = footage
            .iter()
            .zip(analyses.iter())
            .map(|(f, a)| {
                let v = f.metadata.video.as_ref();
                FootageSource {
                    asset_id: f.id.clone(),
                    analysis: a,
                    duration: if f.kind == MediaKind::Image {
                        Tick::from_seconds_f64(3600.0)
                    } else {
                        f.metadata.duration
                    },
                    width: v.map(|v| v.display_width).unwrap_or(1920),
                    height: v.map(|v| v.display_height).unwrap_or(1080),
                }
            })
            .collect();

        // Locks for this run.
        let current: Vec<Clip> = project
            .timeline
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .map(|t| t.clips.clone())
            .unwrap_or_default();
        let originally_locked: Vec<String> = current
            .iter()
            .filter(|c| c.locked)
            .map(|c| c.id.clone())
            .collect();
        let lock_all_but = |keep_free: &dyn Fn(&Clip) -> bool| -> Vec<Clip> {
            current
                .iter()
                .filter(|c| c.locked || !keep_free(c))
                .cloned()
                .map(|mut c| {
                    c.locked = true;
                    c
                })
                .collect()
        };
        let (locked, fx_only, avoid, label): (Vec<Clip>, bool, Vec<(String, u32)>, String) =
            match mode {
                RegenMode::All => (vec![], false, vec![], "Generate".into()),
                RegenMode::KeepLocked => (
                    lock_all_but(&|_| true),
                    false,
                    vec![],
                    "Regenerate unlocked".into(),
                ),
                RegenMode::Range {
                    start_secs,
                    end_secs,
                } => {
                    let (a0, a1) = (
                        Tick::from_seconds_f64(*start_secs),
                        Tick::from_seconds_f64(*end_secs),
                    );
                    if a1 <= a0 {
                        return Err(StudioError::invalid_input("The selected range is empty."));
                    }
                    (
                        lock_all_but(&|c: &Clip| c.start >= a0 && c.end() <= a1),
                        false,
                        vec![],
                        format!("Regenerate {start_secs:.2}–{end_secs:.2}s"),
                    )
                }
                RegenMode::EffectsOnly => (
                    lock_all_but(&|_| false),
                    true,
                    vec![],
                    "Reapply effects".into(),
                ),
                RegenMode::ReplaceClip { clip_id } => {
                    let target = current
                        .iter()
                        .find(|c| &c.id == clip_id)
                        .ok_or_else(|| StudioError::not_found("Clip not found."))?;
                    if target.locked {
                        return Err(StudioError::validation(
                            "Unlock the clip before replacing its scene.",
                        ));
                    }
                    let scene = target
                        .origin
                        .scene_id
                        .as_ref()
                        .and_then(|x| x.rsplit('#').next())
                        .and_then(|x| x.parse::<u32>().ok())
                        .unwrap_or(0);
                    (
                        lock_all_but(&|c: &Clip| &c.id == clip_id),
                        false,
                        vec![(target.asset_id.clone(), scene)],
                        "Replace scene".into(),
                    )
                }
            };
        if !matches!(mode, RegenMode::All) && current.is_empty() {
            return Err(StudioError::invalid_input("Generate an edit first."));
        }
        // Partial modes must keep the same music window and duration as the current edit.
        let (audio_start, seed) = match (mode, &project.plan) {
            (RegenMode::All, _) => (s.audio_start_secs.map(Tick::from_seconds_f64), s.seed),
            (_, Some(pr)) => (
                Some(Tick::from_samples(pr.audio_start_sample, audio.sample_rate)),
                s.seed.wrapping_add(1 + project.versions.len() as u64),
            ),
            (_, None) => (
                s.audio_start_secs.map(Tick::from_seconds_f64),
                s.seed.wrapping_add(1),
            ),
        };
        let duration = if matches!(mode, RegenMode::All) {
            Tick::from_seconds_f64(s.duration_secs)
        } else {
            project.timeline.duration()
        };
        let plan = studio_planner::plan_edit(&PlanInput {
            audio: &audio,
            music_asset_id: music.id.clone(),
            footage: sources,
            preset,
            controls: s.controls.clone(),
            target: OutputTarget {
                width: s.width,
                height: s.height,
                fps: s.fps,
                duration,
            },
            seed,
            variant: s.variant,
            niche,
            cues: s.cues.clone(),
            locked,
            regenerate_locked_effects: fx_only,
            audio_start,
            drop_position: s.drop_position,
            avoid_scenes: avoid,
        })?;
        if !current.is_empty() {
            push_version(project, &format!("Before: {label}"));
        }
        let mut tl = plan.timeline.clone();
        // Temporary locks (range / effects / replace modes) do not stick.
        for t in tl.tracks.iter_mut().filter(|t| t.kind == TrackKind::Video) {
            for c in t.clips.iter_mut() {
                c.locked = originally_locked.contains(&c.id);
            }
        }
        // Keep user track flags (lock/mute/hide) from the current timeline.
        for t in tl.tracks.iter_mut() {
            if let Some(old) = project.timeline.track(&t.id) {
                t.locked = old.locked;
                t.muted = old.muted;
                t.hidden = old.hidden;
            }
        }
        project.timeline = tl;
        project.plan = Some(PlanRecord {
            seed: plan.seed,
            variant: plan.variant,
            preset_id: plan.preset_id.clone(),
            generated_at: now_secs(),
            audio_start_sample: plan.audio_start_sample,
            audio_end_sample: plan.audio_end_sample,
            segments: plan.segments.clone(),
            warnings: plan.warnings.clone(),
        });
        project.touch();
        report(progress, 1.0, "Edit generated");
        Ok(plan)
    }

    // ---------- render / preview ----------

    pub fn render_assets(project: &Project) -> HashMap<String, RenderAsset> {
        project
            .assets
            .iter()
            .map(|a| {
                let v = a.metadata.video.as_ref();
                (
                    a.id.clone(),
                    RenderAsset {
                        path: a.path.clone(),
                        kind: match a.kind {
                            MediaKind::Video => RenderAssetKind::Video,
                            MediaKind::Audio => RenderAssetKind::Audio,
                            MediaKind::Image => RenderAssetKind::Image,
                        },
                        width: v.map(|v| v.display_width).unwrap_or(0),
                        height: v.map(|v| v.display_height).unwrap_or(0),
                        duration: a.metadata.duration,
                        has_audio: a.metadata.has_audio(),
                    },
                )
            })
            .collect()
    }

    /// Resolve "auto" encoders to the best available one.
    pub fn resolve_encoder(&self, requested: &str, prefer_hardware: bool) -> StudioResult<String> {
        if requested != "auto" {
            let list = self.encoders()?;
            return match list.iter().find(|e| e.name == requested) {
                Some(e) if e.available => Ok(requested.to_string()),
                _ => Err(StudioError::new(
                    ErrorCode::EncoderUnavailable,
                    "Rendering failed because the selected encoder is unavailable.",
                )),
            };
        }
        let list = self.encoders()?;
        if prefer_hardware
            && !list
                .iter()
                .any(|e| e.hardware && e.available && e.codec == studio_media::VideoCodec::H264)
        {
            return Err(StudioError::new(
                ErrorCode::EncoderUnavailable,
                "GPU encoding was requested, but no working hardware H.264 encoder was found.",
            ));
        }
        studio_media::encoders::choose_encoder(
            &list,
            studio_media::VideoCodec::H264,
            prefer_hardware,
        )
        .map(|e| e.name.clone())
        .ok_or_else(|| {
            StudioError::new(
                ErrorCode::EncoderUnavailable,
                "No working H.264 encoder was found in this FFmpeg build.",
            )
        })
    }

    /// Export path from the owning profile's destination folder and naming convention.
    pub fn default_export_path(&self, project: &Project) -> PathBuf {
        let profile = self
            .library
            .get_profile(&project.profile_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let dir = profile
            .defaults
            .export_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|d| d.is_absolute())
            .unwrap_or_else(|| self.project_dir(project).join("exports"));
        let naming = if profile.defaults.naming.trim().is_empty() {
            "{project}-{variant}-{n}".to_string()
        } else {
            profile.defaults.naming.clone()
        };
        let date = now_secs() / 86400;
        let mut i = 1;
        loop {
            let name = naming
                .replace("{project}", &project.name)
                .replace("{variant}", project.settings.variant.code())
                .replace("{profile}", &profile.name)
                .replace("{date}", &date.to_string())
                .replace("{n}", &i.to_string());
            let stem = studio_core::ids::sanitize_file_stem(&name);
            let p = dir.join(format!("{stem}.mp4"));
            if !p.exists() {
                return p;
            }
            i += 1;
        }
    }

    pub fn render(
        &self,
        project: &mut Project,
        settings: &RenderSettings,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<RenderReport> {
        self.render_with(
            project,
            settings,
            &ExportOverrides::default(),
            cancel,
            progress,
        )
    }

    /// The timeline actually exported for a set of overrides (range, canvas size, fps).
    /// Canvas changes keep each clip's framing (cover crop around its focus point), so other
    /// aspect ratios are reframed rather than stretched.
    pub fn export_timeline(
        project: &Project,
        o: &ExportOverrides,
    ) -> StudioResult<studio_timeline::Timeline> {
        let mut tl = match o.range {
            Some((a, b)) => project.timeline.subrange(a, b)?,
            None => project.timeline.clone(),
        };
        if let (Some(w), Some(h)) = (o.width, o.height) {
            if w < 16 || h < 16 || w % 2 != 0 || h % 2 != 0 || w > 8192 || h > 8192 {
                return Err(StudioError::invalid_input(
                    "Export resolution must be even, between 16 and 8192 px.",
                ));
            }
            tl.settings.width = w;
            tl.settings.height = h;
        }
        if let Some(f) = o.fps {
            if !(1.0..=240.0).contains(&f.as_f64()) {
                return Err(StudioError::invalid_input("Unsupported frame rate."));
            }
            tl.settings.fps = f;
        }
        Ok(tl)
    }

    pub fn render_with(
        &self,
        project: &mut Project,
        settings: &RenderSettings,
        overrides: &ExportOverrides,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<RenderReport> {
        let tools = self.tools()?;
        let mut s = settings.clone();
        s.encoder = self.resolve_encoder(&settings.encoder, overrides.prefer_hardware)?;
        let assets = Self::render_assets(project);
        let export_tl = Self::export_timeline(project, overrides)?;
        let result =
            studio_render::render_timeline(&tools, &export_tl, &assets, &s, cancel, progress);
        let entry_id = studio_core::ids::new_id("render");
        match &result {
            Ok(r) => {
                project.renders.push(RenderRecord {
                    id: entry_id.clone(),
                    output: r.output.clone(),
                    created_at: now_secs(),
                    encoder: r.encoder.clone(),
                    width: r.width,
                    height: r.height,
                    duration: r.duration,
                    elapsed_secs: r.elapsed_secs,
                    warnings: r.warnings.clone(),
                });
                self.library.add_render(
                    &project.profile_id,
                    &RenderHistoryEntry {
                        id: entry_id,
                        project_id: Some(project.id.clone()),
                        output: r.output.clone(),
                        created_at: now_secs(),
                        ok: true,
                        duration_s: r.duration.as_seconds_f64(),
                        encoder: r.encoder.clone(),
                    },
                )?;
            }
            Err(e) if !e.is_cancelled() => {
                let _ = self.library.add_render(
                    &project.profile_id,
                    &RenderHistoryEntry {
                        id: entry_id,
                        project_id: Some(project.id.clone()),
                        output: s.output.clone(),
                        created_at: now_secs(),
                        ok: false,
                        duration_s: 0.0,
                        encoder: s.encoder.clone(),
                    },
                );
            }
            _ => {}
        }
        result
    }

    pub fn preview_jpeg(&self, project: &Project, t: Tick, scale: f32) -> StudioResult<Vec<u8>> {
        let tools = self.tools()?;
        let assets = Self::render_assets(project);
        let c = studio_render::render_frame(&tools, &project.timeline, &assets, t, scale)?;
        studio_render::pipeline::canvas_to_jpeg(&c, 82)
    }

    /// Low-resolution preview render (same compositor as export).
    pub fn render_preview_file(
        &self,
        project: &Project,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<PathBuf> {
        let tools = self.tools()?;
        let out = self.dirs.cache.join("previews").join(format!(
            "{}-{}.mp4",
            project.id,
            short_hash(&project.timeline)
        ));
        if out.exists() {
            return Ok(out);
        }
        let s = RenderSettings {
            output: out.clone(),
            encoder: self.resolve_encoder("auto", false)?,
            quality: 28,
            speed_preset: "draft".into(),
            scale: 0.5,
            overwrite: true,
            detect_black: false,
            ..Default::default()
        };
        studio_render::render_timeline(
            &tools,
            &project.timeline,
            &Self::render_assets(project),
            &s,
            cancel,
            progress,
        )?;
        Ok(out)
    }
}

/// Regeneration modes (V4 §14). All of them save the previous timeline as a version.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RegenMode {
    /// Fresh edit; ignores locks.
    All,
    /// Keep locked clips, regenerate everything else.
    KeepLocked,
    /// Regenerate only clips fully inside the range; everything else is kept.
    Range { start_secs: f64, end_secs: f64 },
    /// Keep every clip's source and timing; recompute automatic effects.
    EffectsOnly,
    /// Replace one clip's scene, keeping the rest.
    ReplaceClip { clip_id: String },
}

const MAX_VERSIONS: usize = 20;

fn push_version(project: &mut Project, label: &str) {
    let v = TimelineVersion {
        id: studio_core::ids::new_id("ver"),
        label: label.to_string(),
        created_at: now_secs(),
        seed: project
            .plan
            .as_ref()
            .map(|p| p.seed)
            .unwrap_or(project.settings.seed),
        timeline: project.timeline.clone(),
    };
    project.versions.push(v);
    while project.versions.len() > MAX_VERSIONS {
        project.versions.remove(0);
    }
}

/// Apply persistent manual corrections to an analysis.
pub fn apply_audio_edits(a: &mut AudioAnalysis, e: &AudioEdits) {
    let sr = a.sample_rate as f64;
    if e.grid_offset_ms.abs() > 0.0 {
        let d = (e.grid_offset_ms / 1000.0 * sr).round() as i64;
        for b in a.beats.iter_mut() {
            b.sample = (b.sample + d).clamp(0, a.duration_samples);
        }
    }
    let tol = (0.3 * sr) as i64;
    a.drops.retain(|d| {
        !e.removed_drops_secs
            .iter()
            .any(|r| ((r * sr) as i64 - d.sample).abs() <= tol)
    });
    for t in &e.added_drops_secs {
        let sample = (t * sr) as i64;
        if a.drops
            .iter()
            .any(|d| (d.sample - sample).abs() <= tol && d.kind != DropKind::FalseDropCandidate)
        {
            continue;
        }
        let has_main = a.drops.iter().any(|d| d.kind == DropKind::MainDrop);
        a.drops.retain(|d| (d.sample - sample).abs() > tol);
        a.drops.push(DropCandidate {
            sample,
            kind: if has_main {
                DropKind::SecondaryDrop
            } else {
                DropKind::MainDrop
            },
            score: 1.0,
            features: DropFeatures {
                energy_contrast: 0.0,
                low_reentry: 0.0,
                buildup: 0.0,
                pre_drop_dip: 0.0,
                post_density: 0.0,
                sustained: 0.0,
            },
        });
    }
    a.drops.sort_by_key(|d| d.sample);
}

/// Export-time overrides (range, resolution, frame rate, hardware preference).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ExportOverrides {
    pub range: Option<(Tick, Tick)>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<studio_core::Rational>,
    pub prefer_hardware: bool,
}

impl Engine {
    pub fn export_audio(
        &self,
        project: &Project,
        o: &ExportOverrides,
        output: &Path,
        format: &str,
        bitrate_kbps: u32,
        overwrite: bool,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> StudioResult<PathBuf> {
        let tools = self.tools()?;
        let tl = Self::export_timeline(project, o)?;
        studio_render::pipeline::render_audio_only(
            &tools,
            &tl,
            &Self::render_assets(project),
            output,
            format,
            bitrate_kbps,
            overwrite,
            cancel,
            progress,
        )
    }

    pub fn export_still(
        &self,
        project: &Project,
        o: &ExportOverrides,
        t: Tick,
        output: &Path,
        overwrite: bool,
    ) -> StudioResult<PathBuf> {
        let tools = self.tools()?;
        let tl = Self::export_timeline(project, o)?;
        studio_render::pipeline::render_still(
            &tools,
            &tl,
            &Self::render_assets(project),
            t,
            output,
            overwrite,
        )
    }
}

pub fn noop() -> ProgressSink {
    noop_progress()
}
