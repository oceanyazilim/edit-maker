// Thin, typed wrapper over Tauri IPC. All business logic lives in Rust.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AppInfo,
  AssetRole,
  AudioAnalysis,
  Clip,
  ClipQuery,
  DiscoveryResult,
  DeleteReport,
  Profile,
  ProfileAsset,
  ProfileStats,
  RegenMode,
  SegmentSuggestion,
  EditPreset,
  EncoderInfo,
  FfmpegTools,
  JobInfo,
  NicheProfile,
  Project,
  ProjectSettings,
  RecentProject,
  RenderHistoryEntry,
  StudioError,
  Tick,
  TimelineState,
  VideoAnalysis,
  WaveformPeaks,
} from "../types/studio";

export function toStudioError(e: unknown): StudioError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as Record<string, unknown>;
    return { code: String(o.code ?? "internal"), message: String(o.message), details: o.details ? String(o.details) : undefined };
  }
  return { code: "internal", message: typeof e === "string" ? e : "Unexpected error." };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toStudioError(e);
  }
}

export type TimelineOp =
  | { op: "split"; clip_id: string; at: Tick }
  | { op: "trim_start"; clip_id: string; new_start: Tick }
  | { op: "trim_end"; clip_id: string; new_end: Tick }
  | { op: "move"; clip_id: string; track_id: string; new_start: Tick }
  | { op: "delete"; clip_id: string }
  | { op: "ripple_delete"; clip_id: string }
  | { op: "set_track_flags"; track_id: string; locked?: boolean; muted?: boolean; hidden?: boolean }
  | { op: "update_clip"; clip: Clip }
  | { op: "set_clip_locked"; clip_id: string; locked: boolean }
  | { op: "lock_range"; from: Tick; to: Tick; exclusive: boolean }
  | { op: "insert_clip"; track_id: string; asset_id: string; start: Tick; duration?: Tick | null; source_in?: Tick | null }
  | { op: "add_marker"; t: Tick; label: string }
  | { op: "remove_marker"; id: string }
  | { op: "add_track"; kind: "video" | "audio" | "text"; name: string }
  | { op: "undo" }
  | { op: "redo" };

export interface ExportRequest {
  output?: string | null;
  encoder: string;
  quality: number;
  speed_preset: "quality" | "fast" | "draft";
  video_bitrate_kbps?: number | null;
  audio_bitrate_kbps?: number | null;
  overwrite: boolean;
  /** Export-dialog overrides (all optional; omitted = project settings / entire timeline). */
  range_start?: Tick | null;
  range_end?: Tick | null;
  width?: number | null;
  height?: number | null;
  fps?: { num: number; den: number } | null;
  hardware?: "auto" | "gpu" | "cpu";
}

export interface AudioExportRequest {
  output: string;
  format: "m4a" | "wav";
  bitrate_kbps?: number | null;
  overwrite: boolean;
  range_start?: Tick | null;
  range_end?: Tick | null;
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  setFfmpegDir: (dir: string) => call<FfmpegTools>("set_ffmpeg_dir", { dir }),
  detectEncoders: () => call<EncoderInfo[]>("detect_encoders"),
  listPresets: () => call<EditPreset[]>("list_presets"),
  savePreset: (preset: EditPreset) => call<string>("save_preset", { preset }),
  listNiches: () => call<NicheProfile[]>("list_niches"),
  listProfiles: (search = "", includeArchived = false) => call<Profile[]>("list_profiles", { search, includeArchived }),
  activeProfile: () => call<Profile>("active_profile"),
  createProfile: (profile: Profile) => call<Profile>("create_profile", { profile }),
  updateProfile: (profile: Profile) => call<Profile>("update_profile", { profile }),
  duplicateProfile: (profileId: string, name: string) => call<Profile>("duplicate_profile", { profileId, name }),
  archiveProfile: (profileId: string, archived: boolean) => call<void>("archive_profile", { profileId, archived }),
  profileStats: (profileId: string) => call<ProfileStats>("profile_stats", { profileId }),
  deleteProfile: (profileId: string) => call<DeleteReport>("delete_profile", { profileId }),
  switchProfile: (profileId: string, saveCurrent: boolean) => call<Profile>("switch_profile", { profileId, saveCurrent }),
  exportProfileConfig: (profileId: string) => call<string>("export_profile_config", { profileId }),
  defaultProjectSettings: () => call<ProjectSettings>("default_project_settings"),
  favoritePresets: () => call<string[]>("favorite_presets"),
  setFavoritePreset: (presetId: string, favorite: boolean) => call<void>("set_favorite_preset", { presetId, favorite }),
  profileAssets: () => call<ProfileAsset[]>("profile_assets"),
  setAssetShared: (fingerprint: string, shared: boolean) => call<void>("set_asset_shared", { fingerprint, shared }),
  discoverClips: (query: ClipQuery) => call<DiscoveryResult>("discover_clips", { query }),
  recentProjects: () => call<RecentProject[]>("recent_projects"),
  recentRenders: () => call<RenderHistoryEntry[]>("recent_renders"),
  createProject: (name: string, settings: ProjectSettings) => call<Project>("create_project", { name, settings }),
  openProject: (path: string) => call<{ project: Project; autosave_available: boolean; missing_assets: string[] }>("open_project", { path }),
  recoverAutosave: (path: string) => call<Project>("recover_autosave", { path }),
  closeProject: () => call<void>("close_project"),
  getProject: () => call<Project | null>("get_project"),
  saveProject: () => call<string>("save_project"),
  updateSettings: (settings: ProjectSettings) => call<Project>("update_settings", { settings }),
  importMedia: (paths: string[], role: AssetRole) => call<string>("import_media", { paths, role }),
  importFolder: (dir?: string | null) => call<string>("import_folder", { dir: dir ?? null }),
  removeAsset: (assetId: string) => call<Project>("remove_asset", { assetId }),
  relinkAsset: (assetId: string, path: string, allowDifferent: boolean) => call<Project>("relink_asset", { assetId, path, allowDifferent }),
  setAssetRights: (assetId: string, confirmed: boolean, note: string) => call<Project>("set_asset_rights", { assetId, confirmed, note }),
  analyzeAudio: () => call<string>("analyze_audio"),
  getAudioAnalysis: () => call<AudioAnalysis | null>("get_audio_analysis"),
  getWaveform: (assetId: string) => call<WaveformPeaks>("get_waveform", { assetId }),
  analyzeVideo: (assetId: string) => call<string>("analyze_video", { assetId }),
  getVideoAnalysis: (assetId: string) => call<VideoAnalysis | null>("get_video_analysis", { assetId }),
  generateEdit: () => call<string>("generate_edit"),
  regenerateEdit: (mode: RegenMode) => call<string>("regenerate_edit", { mode }),
  restoreVersion: (versionId: string) => call<Project>("restore_version", { versionId }),
  suggestSegments: (durationSecs: number) => call<SegmentSuggestion[]>("suggest_segments", { durationSecs }),
  timelineEdit: (op: TimelineOp) => call<TimelineState>("timeline_edit", { op }),
  previewFrame: async (t: Tick, scale: number): Promise<string> => {
    const buf = await call<ArrayBuffer>("preview_frame", { t, scale });
    return URL.createObjectURL(new Blob([buf], { type: "image/jpeg" }));
  },
  renderPreview: () => call<string>("render_preview"),
  exportVideo: (request: ExportRequest) => call<string>("export_video", { request }),
  exportAudio: (request: AudioExportRequest) => call<string>("export_audio", { request }),
  exportStill: (output: string, t: Tick, overwrite: boolean, width?: number | null, height?: number | null) =>
    call<string>("export_still", { output, t, overwrite, width: width ?? null, height: height ?? null }),
  exportDefaults: () => call<{ dir: string; stem: string }>("export_defaults"),
  pathExists: (path: string) => call<boolean>("path_exists", { path }),
  openPath: (path: string, reveal: boolean) => call<void>("open_path", { path, reveal }),
  listJobs: () => call<JobInfo[]>("list_jobs"),
  cancelJob: (jobId: string) => call<boolean>("cancel_job", { jobId }),
};

/** Wait for a job via polling (usable outside React components). */
export async function waitForJobSafe(id: string): Promise<JobInfo> {
  for (;;) {
    const j = (await api.listJobs()).find((x) => x.id === id);
    if (j && j.status === "completed") return j;
    if (j && (j.status === "failed" || j.status === "cancelled")) throw j.error ?? { code: j.status, message: "The job did not complete." };
    await new Promise((r) => setTimeout(r, 150));
  }
}

export const onJob = (cb: (j: JobInfo) => void): Promise<UnlistenFn> => listen<JobInfo>("job", (e) => cb(e.payload));

export const fileUrl = (path: string) => convertFileSrc(path);

export const dialogs = {
  pickMusic: async () => {
    const r = await open({ multiple: false, filters: [{ name: "Audio", extensions: ["wav", "mp3", "flac", "aac", "m4a", "ogg", "opus"] }] });
    return typeof r === "string" ? r : null;
  },
  pickVideos: async () => {
    const r = await open({ multiple: true, filters: [{ name: "Video & images", extensions: ["mp4", "mov", "mkv", "webm", "avi", "m4v", "png", "jpg", "jpeg"] }] });
    return Array.isArray(r) ? r : r ? [r] : [];
  },
  pickProject: async () => {
    const r = await open({ multiple: false, filters: [{ name: "AI Auto Edit project", extensions: ["aesproj"] }] });
    return typeof r === "string" ? r : null;
  },
  pickFolder: async () => {
    const r = await open({ directory: true, multiple: false });
    return typeof r === "string" ? r : null;
  },
  pickFile: async () => {
    const r = await open({ multiple: false });
    return typeof r === "string" ? r : null;
  },
  saveMp4: async (defaultPath?: string) => save({ defaultPath, filters: [{ name: "MP4 video", extensions: ["mp4"] }] }),
};
