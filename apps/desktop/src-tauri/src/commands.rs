//! IPC commands. Errors are returned as serialized `StudioError`
//! (`{ code, message, details? }`), never as raw panics or stack traces.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use studio_audio::waveform::WaveformPeaks;
use studio_audio::AudioAnalysis;
use studio_core::{CancelToken, StudioError, Tick};
use studio_jobs::{JobInfo, JobKind};
use studio_media::{EncoderInfo, FfmpegTools};
use studio_planner::{EditPreset, NicheProfile, SegmentSuggestion};
use studio_project::engine::{ExportOverrides, RegenMode};
use studio_project::library::{
    DeleteReport, ProfileAsset, ProfileStats, RecentProject, RenderHistoryEntry,
};
use studio_project::{store, AssetRole, MediaAsset, Profile, Project, ProjectSettings};
use studio_render::RenderSettings;
use studio_timeline::{Clip, Timeline};
use studio_vision::VideoAnalysis;

use crate::state::AppState;

type CmdResult<T> = Result<T, StudioError>;

fn no_project() -> StudioError {
    StudioError::invalid_input("No project is open.")
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| StudioError::internal("Background task failed.").with_details(e.to_string()))?
}

fn allow_asset(app: &AppHandle, p: &std::path::Path) {
    let _ = app.asset_protocol_scope().allow_file(p);
}

// ---------------- app ----------------

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    ffmpeg: Option<FfmpegTools>,
    ffmpeg_error: Option<String>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    projects_dir: PathBuf,
    cpu_threads: usize,
}

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let e = state.engine.clone();
    blocking(move || {
        let (ffmpeg, ffmpeg_error) = match e.tools() {
            Ok(t) => (Some(t), None),
            Err(err) => (None, Some(err.message)),
        };
        Ok(AppInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            ffmpeg,
            ffmpeg_error,
            data_dir: e.dirs.data.clone(),
            cache_dir: e.dirs.cache.clone(),
            projects_dir: e.dirs.projects.clone(),
            cpu_threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        })
    })
    .await
}

#[tauri::command]
pub async fn set_ffmpeg_dir(state: State<'_, AppState>, dir: String) -> CmdResult<FfmpegTools> {
    let e = state.engine.clone();
    blocking(move || e.set_ffmpeg_dir(&PathBuf::from(dir))).await
}

#[tauri::command]
pub async fn detect_encoders(state: State<'_, AppState>) -> CmdResult<Vec<EncoderInfo>> {
    let e = state.engine.clone();
    blocking(move || e.encoders()).await
}

#[tauri::command]
pub fn list_presets(state: State<'_, AppState>) -> Vec<EditPreset> {
    state.engine.presets()
}

#[tauri::command]
pub fn save_preset(state: State<'_, AppState>, preset: EditPreset) -> CmdResult<String> {
    Ok(state
        .engine
        .save_user_preset(&preset)?
        .display()
        .to_string())
}

#[tauri::command]
pub fn list_niches(state: State<'_, AppState>) -> Vec<NicheProfile> {
    state.engine.niches()
}

fn active_id(state: &AppState) -> CmdResult<String> {
    state.engine.library.active_profile_id()
}

#[tauri::command]
pub fn recent_projects(state: State<'_, AppState>) -> CmdResult<Vec<RecentProject>> {
    state
        .engine
        .library
        .recent_projects(&active_id(&state)?, 50)
}

#[tauri::command]
pub fn recent_renders(state: State<'_, AppState>) -> CmdResult<Vec<RenderHistoryEntry>> {
    state.engine.library.recent_renders(&active_id(&state)?, 50)
}

// ---------------- profiles ----------------

#[tauri::command]
pub fn list_profiles(
    state: State<'_, AppState>,
    search: Option<String>,
    include_archived: Option<bool>,
) -> CmdResult<Vec<Profile>> {
    state.engine.library.list_profiles(
        include_archived.unwrap_or(false),
        search.as_deref().unwrap_or("").trim(),
    )
}

#[tauri::command]
pub fn active_profile(state: State<'_, AppState>) -> CmdResult<Profile> {
    state.engine.active_profile()
}

#[tauri::command]
pub fn create_profile(state: State<'_, AppState>, profile: Profile) -> CmdResult<Profile> {
    state.engine.library.create_profile(profile)
}

#[tauri::command]
pub fn update_profile(state: State<'_, AppState>, profile: Profile) -> CmdResult<Profile> {
    state.engine.library.update_profile(&profile)
}

#[tauri::command]
pub fn duplicate_profile(
    state: State<'_, AppState>,
    profile_id: String,
    name: String,
) -> CmdResult<Profile> {
    state.engine.library.duplicate_profile(&profile_id, &name)
}

#[tauri::command]
pub fn archive_profile(
    state: State<'_, AppState>,
    profile_id: String,
    archived: bool,
) -> CmdResult<()> {
    state.engine.library.set_archived(&profile_id, archived)
}

#[tauri::command]
pub fn profile_stats(state: State<'_, AppState>, profile_id: String) -> CmdResult<ProfileStats> {
    state.engine.library.profile_stats(&profile_id)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, profile_id: String) -> CmdResult<DeleteReport> {
    let running = state
        .jobs
        .list_for(&profile_id)
        .iter()
        .any(|j| !j.status.is_finished());
    if running {
        return Err(StudioError::validation(
            "This profile still has running jobs. Wait for them to finish or cancel them first.",
        ));
    }
    state.engine.library.delete_profile(&profile_id)
}

/// Switch workspace. Background jobs are NOT cancelled; they keep their owner id.
/// If a project is open it is saved first (`save_current`) or discarded from the
/// view (the autosave file remains for recovery).
#[tauri::command]
pub fn switch_profile(
    state: State<'_, AppState>,
    profile_id: String,
    save_current: bool,
) -> CmdResult<Profile> {
    let target = state
        .engine
        .library
        .get_profile(&profile_id)?
        .ok_or_else(|| StudioError::not_found("Profile not found."))?;
    if target.archived {
        return Err(StudioError::validation(
            "Restore the profile before switching to it.",
        ));
    }
    {
        let mut g = state.project.lock().unwrap();
        if let Some(p) = g.as_mut() {
            if save_current {
                state.engine.save_project(p)?;
            } else if let Err(e) = store::autosave(p) {
                tracing::warn!("autosave before profile switch failed: {}", e.message);
            }
        }
        *g = None;
    }
    state.history.lock().unwrap().clear();
    state.engine.library.set_active_profile(&profile_id)?;
    Ok(target)
}

#[tauri::command]
pub fn export_profile_config(state: State<'_, AppState>, profile_id: String) -> CmdResult<String> {
    let p = state
        .engine
        .library
        .get_profile(&profile_id)?
        .ok_or_else(|| StudioError::not_found("Profile not found."))?;
    let favorites = state.engine.library.favorite_presets(&profile_id)?;
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({ "format": "aes-profile", "version": 1, "profile": p, "favorite_presets": favorites }),
    )?)
}

#[tauri::command]
pub fn default_project_settings(state: State<'_, AppState>) -> CmdResult<ProjectSettings> {
    state.engine.default_project_settings()
}

#[tauri::command]
pub fn favorite_presets(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    state.engine.library.favorite_presets(&active_id(&state)?)
}

#[tauri::command]
pub fn set_favorite_preset(
    state: State<'_, AppState>,
    preset_id: String,
    favorite: bool,
) -> CmdResult<()> {
    state
        .engine
        .library
        .set_favorite_preset(&active_id(&state)?, &preset_id, favorite)
}

#[tauri::command]
pub fn profile_assets(state: State<'_, AppState>) -> CmdResult<Vec<ProfileAsset>> {
    state.engine.library.profile_assets(&active_id(&state)?)
}

#[tauri::command]
pub async fn discover_clips(
    state: State<'_, AppState>,
    query: studio_project::discovery::ClipQuery,
) -> CmdResult<studio_project::discovery::DiscoveryResult> {
    let e = state.engine.clone();
    blocking(move || e.discover_clips(&query)).await
}

#[tauri::command]
pub fn set_asset_shared(
    state: State<'_, AppState>,
    fingerprint: String,
    shared: bool,
) -> CmdResult<()> {
    state
        .engine
        .library
        .set_asset_shared(&active_id(&state)?, &fingerprint, shared)
}

// ---------------- projects ----------------

fn set_current(app: &AppHandle, state: &AppState, p: Project) -> Project {
    for a in &p.assets {
        allow_asset(app, &a.path);
    }
    state.history.lock().unwrap().clear();
    *state.project.lock().unwrap() = Some(p.clone());
    p
}

#[tauri::command]
pub fn create_project(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    settings: ProjectSettings,
) -> CmdResult<Project> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StudioError::invalid_input(
            "Project name must not be empty.",
        ));
    }
    let p = state.engine.create_project(name, settings)?;
    Ok(set_current(&app, &state, p))
}

#[derive(Serialize)]
pub struct OpenResult {
    project: Project,
    autosave_available: bool,
    missing_assets: Vec<String>,
}

#[tauri::command]
pub fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<OpenResult> {
    let r = state.engine.open_project(&PathBuf::from(path))?;
    let project = set_current(&app, &state, r.project);
    Ok(OpenResult {
        project,
        autosave_available: r.autosave_available,
        missing_assets: r.missing_assets,
    })
}

#[tauri::command]
pub fn recover_autosave(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<Project> {
    let p = store::load_autosave(&PathBuf::from(path))?;
    Ok(set_current(&app, &state, p))
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
    state.history.lock().unwrap().clear();
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>) -> Option<Project> {
    state.project.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_project(state: State<'_, AppState>) -> CmdResult<String> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    Ok(state.engine.save_project(p)?.display().to_string())
}

#[tauri::command]
pub fn update_settings(
    state: State<'_, AppState>,
    settings: ProjectSettings,
) -> CmdResult<Project> {
    if settings.width < 16
        || settings.height < 16
        || settings.width % 2 != 0
        || settings.height % 2 != 0
    {
        return Err(StudioError::invalid_input(
            "Resolution must be even and at least 16 px.",
        ));
    }
    if !(1.0..=600.0).contains(&settings.duration_secs) {
        return Err(StudioError::invalid_input(
            "Duration must be between 1 and 600 seconds.",
        ));
    }
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    let res_changed = (p.settings.width, p.settings.height, p.settings.fps)
        != (settings.width, settings.height, settings.fps);
    p.settings = settings;
    if res_changed {
        p.timeline.settings.width = p.settings.width;
        p.timeline.settings.height = p.settings.height;
        p.timeline.settings.fps = p.settings.fps;
    }
    p.touch();
    Ok(p.clone())
}

// ---------------- media ----------------

#[tauri::command]
pub fn import_media(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
    role: AssetRole,
) -> CmdResult<String> {
    if state.project.lock().unwrap().is_none() {
        return Err(no_project());
    }
    let engine = state.engine.clone();
    let project = state.project.clone();
    let label = format!("Import {} file(s)", paths.len());
    Ok(state.jobs.submit_owned(
        Some(active_id(&state)?),
        JobKind::Import,
        label,
        move |cancel, progress| {
            let mut imported: Vec<MediaAsset> = Vec::new();
            let mut errors: Vec<String> = Vec::new();
            let n = paths.len().max(1);
            for (i, p) in paths.iter().enumerate() {
                cancel.check()?;
                studio_core::cancel::report(
                    &progress,
                    i as f32 / n as f32,
                    &format!("Importing {}", p),
                );
                let mut working = project.lock().unwrap().clone().ok_or_else(no_project)?;
                match engine.import_media(&mut working, &PathBuf::from(p), role, &cancel) {
                    Ok(asset) => {
                        allow_asset(&app, &asset.path);
                        let mut g = project.lock().unwrap();
                        if let Some(cur) = g.as_mut() {
                            if !cur.assets.iter().any(|a| {
                                a.id == asset.id
                                    || (a.fingerprint == asset.fingerprint && a.role == asset.role)
                            }) {
                                if role == AssetRole::Music {
                                    cur.assets.retain(|a| a.role != AssetRole::Music);
                                }
                                cur.assets.push(asset.clone());
                                cur.touch();
                            }
                        }
                        imported.push(asset);
                    }
                    Err(e) => errors.push(format!("{}: {}", p, e.message)),
                }
            }
            if imported.is_empty() && !errors.is_empty() {
                return Err(StudioError::invalid_media("No files could be imported.")
                    .with_details(errors.join("\n")));
            }
            Ok(serde_json::json!({ "imported": imported.len(), "errors": errors }))
        },
    ))
}

/// Import all videos of a folder (default: the active profile's media folder).
#[tauri::command]
pub fn import_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    dir: Option<String>,
) -> CmdResult<String> {
    let mut working = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => state
            .engine
            .active_profile()?
            .defaults
            .media_folder
            .map(PathBuf::from)
            .ok_or_else(|| {
                StudioError::invalid_input(
                    "Set a default media folder in the profile settings, or pick a folder.",
                )
            })?,
    };
    let engine = state.engine.clone();
    let project = state.project.clone();
    Ok(state.jobs.submit_owned(Some(active_id(&state)?), JobKind::Import, format!("Import folder {}", dir.display()), move |c, _p| {
        let (imported, skipped, errors) = engine.import_folder(&mut working, &dir, &c)?;
        let mut g = project.lock().unwrap();
        if let Some(cur) = g.as_mut().filter(|cur| cur.id == working.id) {
            for a in &imported {
                allow_asset(&app, &a.path);
                if !cur.assets.iter().any(|x| x.fingerprint == a.fingerprint && x.role == a.role) {
                    cur.assets.push(a.clone());
                }
            }
            cur.touch();
        }
        Ok(serde_json::json!({ "imported": imported.len(), "skipped": skipped, "errors": errors }))
    }))
}

#[tauri::command]
pub fn remove_asset(state: State<'_, AppState>, asset_id: String) -> CmdResult<Project> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    state
        .history
        .lock()
        .unwrap()
        .record("Remove media", &p.timeline);
    p.assets.retain(|a| a.id != asset_id);
    for t in p.timeline.tracks.iter_mut() {
        t.clips.retain(|c| c.asset_id != asset_id);
    }
    p.touch();
    Ok(p.clone())
}

#[tauri::command]
pub fn relink_asset(
    app: AppHandle,
    state: State<'_, AppState>,
    asset_id: String,
    path: String,
    allow_different: bool,
) -> CmdResult<Project> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    store::relink(p, &asset_id, &PathBuf::from(path), allow_different)?;
    if let Some(a) = p.asset(&asset_id) {
        allow_asset(&app, &a.path);
    }
    p.touch();
    Ok(p.clone())
}

#[tauri::command]
pub fn set_asset_rights(
    state: State<'_, AppState>,
    asset_id: String,
    confirmed: bool,
    note: String,
) -> CmdResult<Project> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    let a = p
        .assets
        .iter_mut()
        .find(|a| a.id == asset_id)
        .ok_or_else(|| StudioError::not_found("Asset not found."))?;
    a.rights.confirmed = confirmed;
    a.rights.note = note.chars().take(500).collect();
    p.touch();
    Ok(p.clone())
}

// ---------------- analysis ----------------

fn music_asset(state: &AppState) -> CmdResult<MediaAsset> {
    state
        .project
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(no_project)?
        .music()
        .cloned()
        .ok_or_else(|| StudioError::invalid_input("Import a music track first."))
}

fn asset_by_id(state: &AppState, id: &str) -> CmdResult<MediaAsset> {
    state
        .project
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(no_project)?
        .asset(id)
        .cloned()
        .ok_or_else(|| StudioError::not_found("Asset not found."))
}

#[tauri::command]
pub fn analyze_audio(state: State<'_, AppState>) -> CmdResult<String> {
    let asset = music_asset(&state)?;
    let project_edits = state
        .project
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.settings.audio_edits.clone())
        .unwrap_or_default();
    let engine = state.engine.clone();
    Ok(state.jobs.submit_owned(Some(active_id(&state)?), JobKind::AudioAnalysis, format!("Analyze music: {}", asset.name), move |c, p| {
        let edits = project_edits;
        let a = engine.corrected_audio_analysis(&asset, &edits, &c, &p)?;
        Ok(serde_json::json!({ "bpm": a.tempo.bpm, "beats": a.beats.len(), "kicks": a.kicks.len(), "drops": a.drops.len(), "warnings": a.warnings }))
    }))
}

#[tauri::command]
pub async fn get_audio_analysis(state: State<'_, AppState>) -> CmdResult<Option<AudioAnalysis>> {
    let asset = match music_asset(&state) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    let edits = state
        .project
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.settings.audio_edits.clone())
        .unwrap_or_default();
    let e = state.engine.clone();
    blocking(move || {
        // Only return an analysis that exists (cached); never start a long analysis here.
        let cfg = studio_audio::AnalysisConfig {
            bpm_override: edits.bpm_override,
            ..Default::default()
        };
        if e.cached_audio_analysis_cfg(&asset, &cfg).is_none() {
            return Ok(None);
        }
        e.corrected_audio_analysis(
            &asset,
            &edits,
            &CancelToken::new(),
            &studio_core::cancel::noop_progress(),
        )
        .map(Some)
    })
    .await
}

#[tauri::command]
pub async fn get_waveform(
    state: State<'_, AppState>,
    asset_id: String,
) -> CmdResult<WaveformPeaks> {
    let asset = asset_by_id(&state, &asset_id)?;
    let e = state.engine.clone();
    blocking(move || e.waveform(&asset, &CancelToken::new())).await
}

#[tauri::command]
pub fn analyze_video(state: State<'_, AppState>, asset_id: String) -> CmdResult<String> {
    let asset = asset_by_id(&state, &asset_id)?;
    let engine = state.engine.clone();
    Ok(state.jobs.submit_owned(
        Some(active_id(&state)?),
        JobKind::VideoAnalysis,
        format!("Analyze scenes: {}", asset.name),
        move |c, p| {
            let a = engine.video_analysis(&asset, &c, &p)?;
            Ok(serde_json::json!({ "scenes": a.scenes.len(), "cuts": a.cuts.len() }))
        },
    ))
}

#[tauri::command]
pub async fn get_video_analysis(
    state: State<'_, AppState>,
    asset_id: String,
) -> CmdResult<Option<VideoAnalysis>> {
    let asset = asset_by_id(&state, &asset_id)?;
    let e = state.engine.clone();
    blocking(move || Ok(e.cached_video_analysis(&asset))).await
}

// ---------------- generation & timeline ----------------

#[tauri::command]
pub fn generate_edit(state: State<'_, AppState>) -> CmdResult<String> {
    regenerate_edit(state, RegenMode::All)
}

/// Generate / regenerate with locks (V4 §14). The engine saves the previous timeline as a
/// version and the change is also recorded in the undo history.
#[tauri::command]
pub fn regenerate_edit(state: State<'_, AppState>, mode: RegenMode) -> CmdResult<String> {
    let working = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let engine = state.engine.clone();
    let project = state.project.clone();
    let history = state.history.clone();
    let label = match &mode {
        RegenMode::All => "Generate edit".to_string(),
        RegenMode::KeepLocked => "Regenerate unlocked clips".to_string(),
        RegenMode::Range {
            start_secs,
            end_secs,
        } => format!("Regenerate {start_secs:.2}–{end_secs:.2}s"),
        RegenMode::EffectsOnly => "Reapply effects".to_string(),
        RegenMode::ReplaceClip { .. } => "Replace scene".to_string(),
    };
    Ok(state.jobs.submit_owned(Some(active_id(&state)?), JobKind::Generate, label.clone(), move |c, p| {
        let mut w = working;
        let plan = engine.regenerate(&mut w, &mode, &c, &p)?;
        let mut g = project.lock().unwrap();
        let cur = g
            .as_mut()
            .filter(|cur| cur.id == w.id)
            .ok_or_else(|| StudioError::invalid_input("The project was closed during generation."))?;
        history.lock().unwrap().record(label, &cur.timeline);
        cur.timeline = w.timeline;
        cur.plan = w.plan;
        cur.versions = w.versions;
        cur.touch();
        Ok(serde_json::json!({ "clips": plan.timeline.tracks.first().map(|t| t.clips.len()).unwrap_or(0), "warnings": plan.warnings, "cues": plan.cues }))
    }))
}

#[tauri::command]
pub fn restore_version(state: State<'_, AppState>, version_id: String) -> CmdResult<Project> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    state
        .history
        .lock()
        .unwrap()
        .record("Restore version", &p.timeline);
    state.engine.restore_version(p, &version_id)?;
    Ok(p.clone())
}

#[tauri::command]
pub async fn suggest_segments(
    state: State<'_, AppState>,
    duration_secs: f64,
) -> CmdResult<Vec<SegmentSuggestion>> {
    let project = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    if !(1.0..=600.0).contains(&duration_secs) {
        return Err(StudioError::invalid_input(
            "Duration must be between 1 and 600 seconds.",
        ));
    }
    let e = state.engine.clone();
    blocking(move || e.suggest_segments(&project, duration_secs)).await
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TimelineOp {
    Split {
        clip_id: String,
        at: i64,
    },
    TrimStart {
        clip_id: String,
        new_start: i64,
    },
    TrimEnd {
        clip_id: String,
        new_end: i64,
    },
    Move {
        clip_id: String,
        track_id: String,
        new_start: i64,
    },
    Delete {
        clip_id: String,
    },
    RippleDelete {
        clip_id: String,
    },
    SetTrackFlags {
        track_id: String,
        locked: Option<bool>,
        muted: Option<bool>,
        hidden: Option<bool>,
    },
    UpdateClip {
        clip: Clip,
    },
    SetClipLocked {
        clip_id: String,
        locked: bool,
    },
    /// Insert media at `start` on a track (duration defaults to min(3 s, remaining source)).
    InsertClip {
        track_id: String,
        asset_id: String,
        start: i64,
        duration: Option<i64>,
        source_in: Option<i64>,
    },
    AddMarker {
        t: i64,
        label: String,
    },
    RemoveMarker {
        id: String,
    },
    AddTrack {
        kind: studio_timeline::TrackKind,
        name: String,
    },
    /// Lock all clips fully inside [from, to) on the first video track (exclusive = unlock others).
    LockRange {
        from: i64,
        to: i64,
        exclusive: bool,
    },
    Undo,
    Redo,
}

#[derive(Serialize)]
pub struct TimelineState {
    timeline: Timeline,
    can_undo: bool,
    can_redo: bool,
    undo_label: Option<String>,
    redo_label: Option<String>,
}

#[tauri::command]
pub fn timeline_edit(state: State<'_, AppState>, op: TimelineOp) -> CmdResult<TimelineState> {
    let mut g = state.project.lock().unwrap();
    let p = g.as_mut().ok_or_else(no_project)?;
    let mut h = state.history.lock().unwrap();
    let before = p.timeline.clone();
    let label: Option<&str> = match &op {
        TimelineOp::InsertClip {
            track_id,
            asset_id,
            start,
            duration,
            source_in,
        } => {
            let asset = p
                .asset(asset_id)
                .cloned()
                .ok_or_else(|| StudioError::not_found("Asset not found."))?;
            let is_image = asset.kind == studio_media::MediaKind::Image;
            let src_in = Tick(source_in.unwrap_or(0)).max(Tick::ZERO);
            let avail = if is_image {
                Tick::from_seconds_f64(3600.0)
            } else {
                asset.metadata.duration - src_in
            };
            if avail <= Tick::ZERO {
                return Err(StudioError::validation(
                    "The source in-point is beyond the end of the media.",
                ));
            }
            let dur = duration
                .map(Tick)
                .unwrap_or(Tick::from_seconds_f64(3.0))
                .min(avail);
            let mut clip = Clip::new(
                studio_core::ids::new_id("clip"),
                asset_id.clone(),
                Tick(*start),
                dur,
                src_in,
            );
            clip.origin.reason = Some("added manually".into());
            p.timeline.insert_clip(track_id, clip)?;
            Some("Insert clip")
        }
        TimelineOp::AddMarker { t, label } => {
            p.timeline.add_marker(
                Tick(*t),
                label.chars().take(60).collect::<String>(),
                studio_core::ids::new_id("m"),
            )?;
            Some("Add marker")
        }
        TimelineOp::RemoveMarker { id } => {
            p.timeline.remove_marker(id)?;
            Some("Remove marker")
        }
        TimelineOp::AddTrack { kind, name } => {
            p.timeline
                .add_track(*kind, name.chars().take(40).collect::<String>());
            Some("Add track")
        }
        TimelineOp::SetClipLocked { clip_id, locked } => {
            p.timeline.set_clip_locked(clip_id, *locked)?;
            Some(if *locked { "Lock clip" } else { "Unlock clip" })
        }
        TimelineOp::LockRange {
            from,
            to,
            exclusive,
        } => {
            let tid = p
                .timeline
                .video_tracks()
                .next()
                .map(|t| t.id.clone())
                .ok_or_else(|| StudioError::not_found("No video track."))?;
            p.timeline
                .lock_range(&tid, Tick(*from), Tick(*to), *exclusive)?;
            Some("Lock range")
        }
        TimelineOp::Undo => {
            h.undo(&mut p.timeline);
            None
        }
        TimelineOp::Redo => {
            h.redo(&mut p.timeline);
            None
        }
        TimelineOp::Split { clip_id, at } => {
            p.timeline
                .split_clip(clip_id, Tick(*at), studio_core::ids::new_id("clip"))?;
            Some("Split clip")
        }
        TimelineOp::TrimStart { clip_id, new_start } => {
            p.timeline.trim_start(clip_id, Tick(*new_start))?;
            Some("Trim start")
        }
        TimelineOp::TrimEnd { clip_id, new_end } => {
            let max = p
                .timeline
                .clip(clip_id)
                .and_then(|c| p.asset(&c.asset_id))
                .filter(|a| a.kind != studio_media::MediaKind::Image)
                .map(|a| a.metadata.duration);
            p.timeline.trim_end(clip_id, Tick(*new_end), max)?;
            Some("Trim end")
        }
        TimelineOp::Move {
            clip_id,
            track_id,
            new_start,
        } => {
            p.timeline.move_clip(clip_id, track_id, Tick(*new_start))?;
            Some("Move clip")
        }
        TimelineOp::Delete { clip_id } => {
            p.timeline.remove_clip(clip_id)?;
            Some("Delete clip")
        }
        TimelineOp::RippleDelete { clip_id } => {
            p.timeline.ripple_delete(clip_id)?;
            Some("Ripple delete")
        }
        TimelineOp::SetTrackFlags {
            track_id,
            locked,
            muted,
            hidden,
        } => {
            p.timeline
                .set_track_flags(track_id, *locked, *muted, *hidden)?;
            Some("Track settings")
        }
        TimelineOp::UpdateClip { clip } => {
            for e in &clip.effects {
                e.kind.validate()?;
            }
            if !(clip.speed > 0.0 && clip.speed <= 16.0) || !(0.0..=4.0).contains(&clip.volume) {
                return Err(StudioError::validation(
                    "Clip speed or volume is out of range.",
                ));
            }
            let (ti, ci) = p
                .timeline
                .find_clip(&clip.id)
                .ok_or_else(|| StudioError::not_found("Clip not found."))?;
            if p.timeline.tracks[ti].locked {
                return Err(StudioError::validation("The track is locked."));
            }
            let old = p.timeline.tracks[ti].clips[ci].clone();
            // A speed/source change must stay inside the source media (images are unbounded).
            if (clip.speed - old.speed).abs() > 1e-6 || clip.source_in != old.source_in {
                if let Some(asset) = p.asset(&clip.asset_id) {
                    if asset.kind != studio_media::MediaKind::Image {
                        let need = clip.source_in
                            + Tick::from_seconds_f64(old.duration.as_seconds_f64() * clip.speed);
                        if clip.source_in < Tick::ZERO
                            || need > asset.metadata.duration + Tick::from_seconds_f64(0.002)
                        {
                            return Err(StudioError::validation(format!(
                                "At {:.2}x this clip would need {:.2}s of source, but the media is {:.2}s long. Shorten the clip or lower the speed.",
                                clip.speed,
                                need.as_seconds_f64(),
                                asset.metadata.duration.as_seconds_f64()
                            )));
                        }
                    }
                }
            }
            // Position/duration changes go through dedicated ops; keep them fixed here.
            let mut c = clip.clone();
            c.start = old.start;
            c.duration = old.duration;
            p.timeline.tracks[ti].clips[ci] = c;
            Some("Edit clip")
        }
    };
    if let Some(l) = label {
        h.record(l, &before);
    }
    if label.is_some() || matches!(op, TimelineOp::Undo | TimelineOp::Redo) {
        p.touch();
    }
    Ok(TimelineState {
        timeline: p.timeline.clone(),
        can_undo: h.can_undo(),
        can_redo: h.can_redo(),
        undo_label: h.undo_label().map(String::from),
        redo_label: h.redo_label().map(String::from),
    })
}

// ---------------- preview & export ----------------

#[tauri::command]
pub async fn preview_frame(
    state: State<'_, AppState>,
    t: i64,
    scale: f32,
) -> Result<tauri::ipc::Response, StudioError> {
    let project = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let e = state.engine.clone();
    let scale = scale.clamp(0.1, 1.0);
    let bytes = blocking(move || e.preview_jpeg(&project, Tick(t), scale)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn render_preview(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    let project = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let engine = state.engine.clone();
    Ok(state.jobs.submit_owned(
        Some(active_id(&state)?),
        JobKind::Preview,
        "Render preview",
        move |c, p| {
            let path = engine.render_preview_file(&project, &c, &p)?;
            allow_asset(&app, &path);
            Ok(path.display().to_string())
        },
    ))
}

#[derive(Deserialize)]
pub struct ExportRequest {
    output: Option<String>,
    encoder: String,
    quality: u8,
    speed_preset: String,
    video_bitrate_kbps: Option<u32>,
    audio_bitrate_kbps: Option<u32>,
    overwrite: bool,
    /// Optional export-time overrides (export dialog).
    #[serde(default)]
    range_start: Option<i64>,
    #[serde(default)]
    range_end: Option<i64>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    fps: Option<studio_core::Rational>,
    /// "auto" | "gpu" | "cpu"
    #[serde(default)]
    hardware: Option<String>,
}

fn overrides_of(r: &ExportRequest) -> CmdResult<ExportOverrides> {
    let range = match (r.range_start, r.range_end) {
        (Some(a), Some(b)) if b > a => Some((Tick(a), Tick(b))),
        (None, None) => None,
        _ => return Err(StudioError::invalid_input("The export range is invalid.")),
    };
    Ok(ExportOverrides {
        range,
        width: r.width,
        height: r.height,
        fps: r.fps,
        prefer_hardware: r.hardware.as_deref() == Some("gpu"),
    })
}

/// Reject Windows-illegal file names before anything is rendered.
fn validate_output(path: &std::path::Path, exts: &[&str]) -> CmdResult<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3",
    ];
    if name.is_empty()
        || name.chars().any(|c| {
            matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 32
        })
        || name.ends_with(' ')
        || name.ends_with('.')
        || RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&stem))
    {
        return Err(StudioError::invalid_input(
            "The file name contains characters that Windows does not allow.",
        ));
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !exts.contains(&ext.as_str()) {
        return Err(StudioError::invalid_input(format!(
            "The output file must use one of: .{}",
            exts.join(", .")
        )));
    }
    Ok(())
}

#[tauri::command]
pub fn export_video(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExportRequest,
) -> CmdResult<String> {
    let working = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    if working.timeline.duration() <= Tick::ZERO {
        return Err(StudioError::validation(
            "The timeline is empty. Generate or build an edit first.",
        ));
    }
    let engine = state.engine.clone();
    let project = state.project.clone();
    let output = request
        .output
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| engine.default_export_path(&working));
    validate_output(&output, &["mp4"])?;
    let overrides = overrides_of(&request)?;
    let encoder = if request.hardware.as_deref() == Some("cpu") && request.encoder == "auto" {
        "libx264".to_string()
    } else {
        request.encoder.clone()
    };
    let settings = RenderSettings {
        output,
        encoder,
        quality: request.quality.clamp(0, 51),
        speed_preset: request.speed_preset,
        video_bitrate_kbps: request.video_bitrate_kbps,
        audio_bitrate_kbps: request.audio_bitrate_kbps.unwrap_or(192).clamp(64, 512),
        overwrite: request.overwrite,
        ..Default::default()
    };
    let label = format!(
        "Export {}",
        settings
            .output
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    Ok(state.jobs.submit_owned(
        Some(active_id(&state)?),
        JobKind::Render,
        label,
        move |c, p| {
            let mut w = working;
            let rep = engine.render_with(&mut w, &settings, &overrides, &c, &p)?;
            allow_asset(&app, &rep.output);
            if let Some(cur) = project
                .lock()
                .unwrap()
                .as_mut()
                .filter(|cur| cur.id == w.id)
            {
                cur.renders = w.renders.clone();
                cur.touch();
            }
            Ok(rep)
        },
    ))
}

/// Default export folder and file stem for the open project (export dialog).
#[tauri::command]
pub fn export_defaults(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let project = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let p = state.engine.default_export_path(&project);
    let dir = p
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".into());
    Ok(serde_json::json!({ "dir": dir, "stem": stem }))
}

/// Whether a file already exists (export dialog overwrite warning).
#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

#[derive(Deserialize)]
pub struct AudioExportRequest {
    output: String,
    /// "m4a" | "wav"
    format: String,
    bitrate_kbps: Option<u32>,
    overwrite: bool,
    range_start: Option<i64>,
    range_end: Option<i64>,
}

#[tauri::command]
pub fn export_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    request: AudioExportRequest,
) -> CmdResult<String> {
    let working = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let output = PathBuf::from(&request.output);
    let fmt = if request.format == "wav" {
        "wav"
    } else {
        "m4a"
    };
    validate_output(&output, &[fmt])?;
    let range = match (request.range_start, request.range_end) {
        (Some(a), Some(b)) if b > a => Some((Tick(a), Tick(b))),
        (None, None) => None,
        _ => return Err(StudioError::invalid_input("The export range is invalid.")),
    };
    let o = ExportOverrides {
        range,
        ..Default::default()
    };
    let engine = state.engine.clone();
    let label = format!(
        "Export audio {}",
        output
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let bitrate = request.bitrate_kbps.unwrap_or(192).clamp(64, 512);
    let overwrite = request.overwrite;
    let fmt = fmt.to_string();
    Ok(state.jobs.submit_owned(
        Some(active_id(&state)?),
        JobKind::Render,
        label,
        move |c, p| {
            let out =
                engine.export_audio(&working, &o, &output, &fmt, bitrate, overwrite, &c, &p)?;
            allow_asset(&app, &out);
            Ok(serde_json::json!({ "output": out }))
        },
    ))
}

#[tauri::command]
pub async fn export_still(
    app: AppHandle,
    state: State<'_, AppState>,
    output: String,
    t: i64,
    overwrite: bool,
    width: Option<u32>,
    height: Option<u32>,
) -> CmdResult<String> {
    let project = state
        .project
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(no_project)?;
    let out = PathBuf::from(output);
    validate_output(&out, &["png", "jpg", "jpeg"])?;
    let e = state.engine.clone();
    let o = ExportOverrides {
        width,
        height,
        ..Default::default()
    };
    let path = blocking(move || e.export_still(&project, &o, Tick(t), &out, overwrite)).await?;
    allow_asset(&app, &path);
    Ok(path.display().to_string())
}

/// Open an exported file or reveal it in Explorer. Only existing files with media/image
/// extensions are accepted; arguments are passed without a shell.
#[tauri::command]
pub fn open_path(path: String, reveal: bool) -> CmdResult<()> {
    let p = std::fs::canonicalize(&path)
        .map_err(|_| StudioError::not_found("The file no longer exists."))?;
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !p.is_file() || !["mp4", "m4a", "wav", "png", "jpg", "jpeg"].contains(&ext.as_str()) {
        return Err(StudioError::invalid_input(
            "Only exported media files can be opened.",
        ));
    }
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").map(|x| x.to_string()).unwrap_or(s);
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer.exe");
        if reveal {
            cmd.arg(format!("/select,{s}"));
        } else {
            cmd.arg(&s);
        }
        cmd.spawn().map_err(|e| {
            StudioError::internal("Explorer could not be started.").with_details(e.to_string())
        })?;
    }
    #[cfg(not(windows))]
    {
        let _ = (reveal, s);
        return Err(StudioError::internal(
            "Opening files is only implemented on Windows.",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.jobs.list()
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: String) -> bool {
    state.jobs.cancel(&job_id)
}

#[allow(dead_code)]
fn _assert_send(_: Arc<AppState>) {}
