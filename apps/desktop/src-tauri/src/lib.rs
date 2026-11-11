//! Tauri shell. The frontend is a view over the Rust engine: every command
//! validates input, long work runs as background jobs, and job progress is
//! pushed to the UI as `job` events.

mod commands;
mod state;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::state::AppState;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::new().map_err(|e| format!("{}: {:?}", e.message, e.details))?;
            // Asset protocol (thumbnails, previews, exports, imported media) is limited to
            // application directories plus explicitly imported files.
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(&state.engine.dirs.cache, true);
            let _ = scope.allow_directory(&state.engine.dirs.projects, true);

            let handle = app.handle().clone();
            state.jobs.set_listener(Arc::new(move |info| {
                let _ = handle.emit("job", info);
            }));

            // Autosave every 30 s when the project changed since the last autosave.
            let p = state.project.clone();
            std::thread::spawn(move || {
                let mut last = 0u64;
                loop {
                    std::thread::sleep(Duration::from_secs(30));
                    let snapshot = { p.lock().unwrap().clone() };
                    if let Some(proj) = snapshot {
                        if proj.modified_at > last {
                            if let Err(e) = studio_project::store::autosave(&proj) {
                                tracing::warn!("autosave failed: {}", e.message);
                            }
                            last = proj.modified_at;
                        }
                    }
                }
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::set_ffmpeg_dir,
            commands::detect_encoders,
            commands::list_presets,
            commands::save_preset,
            commands::list_niches,
            commands::recent_projects,
            commands::recent_renders,
            commands::list_profiles,
            commands::active_profile,
            commands::create_profile,
            commands::update_profile,
            commands::duplicate_profile,
            commands::archive_profile,
            commands::profile_stats,
            commands::delete_profile,
            commands::switch_profile,
            commands::export_profile_config,
            commands::default_project_settings,
            commands::favorite_presets,
            commands::set_favorite_preset,
            commands::profile_assets,
            commands::set_asset_shared,
            commands::discover_clips,
            commands::create_project,
            commands::open_project,
            commands::recover_autosave,
            commands::close_project,
            commands::get_project,
            commands::save_project,
            commands::update_settings,
            commands::import_media,
            commands::import_folder,
            commands::remove_asset,
            commands::relink_asset,
            commands::set_asset_rights,
            commands::analyze_audio,
            commands::get_audio_analysis,
            commands::get_waveform,
            commands::analyze_video,
            commands::get_video_analysis,
            commands::generate_edit,
            commands::regenerate_edit,
            commands::restore_version,
            commands::suggest_segments,
            commands::timeline_edit,
            commands::preview_frame,
            commands::render_preview,
            commands::export_video,
            commands::export_audio,
            commands::export_still,
            commands::open_path,
            commands::export_defaults,
            commands::path_exists,
            commands::list_jobs,
            commands::cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Auto Edit Studio");
}
