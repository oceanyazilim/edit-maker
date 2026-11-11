// Typed mirror of the Rust serde models (crates/*). Ticks are integers:
// 1 second = 705_600_000 ticks. Keep in sync with the Rust definitions;
// see docs/ARCHITECTURE.md §Contracts.

export type Tick = number;
export interface Rational {
  num: number;
  den: number;
}

export interface StudioError {
  code: string;
  message: string;
  details?: string;
}

// ---- media ----
export type MediaKind = "video" | "audio" | "image";
export type AssetRole = "music" | "footage" | "overlay" | "reference";

export interface VideoStreamInfo {
  index: number;
  codec: string;
  width: number;
  height: number;
  display_width: number;
  display_height: number;
  rotation: number;
  fps: Rational;
  variable_frame_rate: boolean;
  pix_fmt?: string | null;
  hdr: boolean;
  bit_rate?: number | null;
}
export interface AudioStreamInfo {
  index: number;
  codec: string;
  sample_rate: number;
  channels: number;
  channel_layout?: string | null;
}
export interface MediaMetadata {
  kind: MediaKind;
  container: string;
  duration: Tick;
  size_bytes: number;
  video?: VideoStreamInfo | null;
  audio: AudioStreamInfo[];
  warnings: string[];
}
export interface MediaAsset {
  id: string;
  path: string;
  name: string;
  fingerprint: string;
  kind: MediaKind;
  role: AssetRole;
  metadata: MediaMetadata;
  rights: { confirmed: boolean; note: string };
  thumbnail?: string | null;
  proxy?: string | null;
  missing: boolean;
  imported_at: number;
}

// ---- timeline ----
export type TrackKind = "video" | "audio" | "text";
export type FitMode = "cover" | "fit";
export type TransitionKind = "cross_dissolve" | "flash_cut" | "zoom_blend" | "dip_to_black";
export type EffectKind =
  | { type: "zoom_punch"; amount: number }
  | { type: "shake"; amplitude: number; frequency_hz: number; seed: number }
  | { type: "flash"; strength: number }
  | { type: "rgb_split"; offset: number }
  | { type: "color"; brightness: number; contrast: number; saturation: number }
  | { type: "vignette"; strength: number }
  | { type: "fade"; fade_in: boolean }
  | { type: "glitch"; strength: number; seed: number }
  | { type: "chromatic_aberration"; amount: number }
  | { type: "motion_blur"; amount: number; angle_deg: number }
  | { type: "zoom_blur"; amount: number };

export interface EffectInstance {
  id: string;
  kind: EffectKind;
  offset: Tick;
  duration: Tick;
  version: number;
  trigger?: string | null;
}
export interface Framing {
  mode: FitMode;
  center_x: number;
  center_y: number;
  zoom: number;
  rotation_deg: number;
}
export interface Clip {
  id: string;
  asset_id: string;
  start: Tick;
  duration: Tick;
  source_in: Tick;
  speed: number;
  volume: number;
  framing: Framing;
  keyframes: { t: Tick; zoom: number; offset_x: number; offset_y: number; ease: string }[];
  effects: EffectInstance[];
  transition_in?: { kind: TransitionKind; duration: Tick } | null;
  fade_in: Tick;
  fade_out: Tick;
  origin: { section?: string | null; reason?: string | null; scene_id?: string | null; score?: number | null };
  text?: string | null;
  locked: boolean;
}
export interface Track {
  id: string;
  kind: TrackKind;
  name: string;
  clips: Clip[];
  locked: boolean;
  muted: boolean;
  hidden: boolean;
}
export interface Marker {
  id: string;
  t: Tick;
  label: string;
  kind: "user" | "beat" | "downbeat" | "kick" | "drop" | "section";
}
export interface Timeline {
  schema_version: number;
  settings: { width: number; height: number; fps: Rational; sample_rate: number; background: number[] };
  tracks: Track[];
  markers: Marker[];
  duration?: Tick | null;
}
export interface TimelineState {
  timeline: Timeline;
  can_undo: boolean;
  can_redo: boolean;
  undo_label?: string | null;
  redo_label?: string | null;
}

// ---- planner ----
export type Variant = "balanced" | "maximum_impact" | "smooth_cinematic" | "high_motion" | "minimal_clean" | "experimental";
export interface CreativeControls {
  beat_sync_strength: number;
  cut_density: number;
  drop_impact: number;
  effect_intensity: number;
  scene_continuity: number;
  motion_energy: number;
  aggressiveness: number;
  transition_frequency: number;
}
export interface EditPreset {
  schema_version: number;
  id: string;
  name: string;
  version: number;
  description: string;
  builtin: boolean;
  pacing: { base_shot_beats: number; min_shot_secs: number; max_shot_secs: number; build_up_acceleration: number; section_multipliers: Record<string, number> };
  beat_sync: number;
  drop: { impact: number; position: number; flash: boolean; zoom_punch: number; rgb_split: boolean; shake: boolean; pre_drop_hold_beats: number; pre_drop_slowmo: number; velocity: number; glitch: number; zoom_blur: number };
  effects: { intensity: number; kick_zoom: number; kick_shake: number; kick_threshold: number; cooldown_beats: number; max_per_bar: number; kick_sections: string[]; vignette: number; slow_zoom: number; color: { brightness: number; contrast: number; saturation: number } };
  transitions: { frequency: number; kinds: string[]; duration_beats: number; sections: string[] };
  energy_bias: number;
  smart_crop: boolean;
  fade_in_secs: number;
  fade_out_secs: number;
  output: { platforms: string[]; aspect: string; duration_secs: number };
  tags: string[];
  [k: string]: unknown;
}
export interface NicheProfile {
  id: string;
  name: string;
  category: string;
  description: string;
  tag_weights: Record<string, number>;
  default_preset: string;
  cut_density_bias: number;
  effect_intensity_bias: number;
  transition_bias: number;
  energy_bias: number;
  recommended_aspects: string[];
  recommended_duration_secs: number;
  suggested_music: string[];
}
export interface PlannedSegment {
  start: Tick;
  end: Tick;
  section: string;
  target_energy: number;
  asset_id: string;
  scene_index: number;
  source_in: Tick;
  speed: number;
  score: number;
  reason: string;
  is_drop: boolean;
}

// ---- project ----
export interface Cue {
  at_secs: number;
  asset_id?: string | null;
  source_secs?: number | null;
  snap_to_beat: boolean;
  label: string;
}
export interface ProjectSettings {
  platform: string;
  aspect: string;
  width: number;
  height: number;
  fps: Rational;
  duration_secs: number;
  niche_id?: string | null;
  preset_id: string;
  variant: Variant;
  seed: number;
  controls: CreativeControls;
  cues: Cue[];
  audio_start_secs?: number | null;
  drop_position?: number | null;
  audio_edits: AudioEdits;
}
export interface AudioEdits {
  bpm_override?: number | null;
  grid_offset_ms: number;
  added_drops_secs: number[];
  removed_drops_secs: number[];
}
export interface TimelineVersion {
  id: string;
  label: string;
  created_at: number;
  seed: number;
  timeline: Timeline;
}
export interface SegmentSuggestion {
  kind: "main_drop_edit" | "second_drop_edit" | "build_up_to_drop" | "high_energy" | "cinematic_intro";
  label: string;
  start_secs: number;
  end_secs: number;
  drop_offset_secs?: number | null;
  score: number;
  phrase_fit_duration_secs?: number | null;
  reasons: string[];
}
export type RegenMode =
  | { mode: "all" }
  | { mode: "keep_locked" }
  | { mode: "range"; start_secs: number; end_secs: number }
  | { mode: "effects_only" }
  | { mode: "replace_clip"; clip_id: string };
export interface RenderRecord {
  id: string;
  output: string;
  created_at: number;
  encoder: string;
  width: number;
  height: number;
  duration: Tick;
  elapsed_secs: number;
  warnings: string[];
}
export interface Project {
  schema_version: number;
  id: string;
  name: string;
  created_at: number;
  modified_at: number;
  settings: ProjectSettings;
  assets: MediaAsset[];
  timeline: Timeline;
  plan?: { seed: number; variant: Variant; preset_id: string; generated_at: number; segments: PlannedSegment[]; warnings: string[]; audio_start_sample: number; audio_end_sample: number } | null;
  renders: RenderRecord[];
  versions: TimelineVersion[];
}
export interface RecentProject {
  id: string;
  name: string;
  path: string;
  modified_at: number;
}
export interface RenderHistoryEntry {
  id: string;
  project_id?: string | null;
  output: string;
  created_at: number;
  ok: boolean;
  duration_s: number;
  encoder: string;
}

// ---- analysis ----
export type SectionKind = "intro" | "groove" | "build_up" | "drop" | "breakdown" | "outro";
export interface AudioAnalysis {
  version: number;
  sample_rate: number;
  duration_samples: number;
  channels: number;
  tempo: { bpm: number; confidence: number; alternatives: { bpm: number; salience: number }[]; overridden: boolean };
  downbeat_confidence: number;
  beats: { sample: number; strength: number; is_downbeat: boolean; bar: number; beat_in_bar: number }[];
  kicks: { sample: number; intensity: number; confidence: number }[];
  snares: { sample: number; intensity: number; confidence: number }[];
  sections: { start_sample: number; end_sample: number; kind: SectionKind; energy: number; energy_slope: number; confidence: number }[];
  drops: { sample: number; kind: "main_drop" | "secondary_drop" | "false_drop_candidate"; score: number }[];
  silences: { start_sample: number; end_sample: number }[];
  curves: { hop_samples: number; rms_db: number[]; low_db: number[]; mid_db: number[]; high_db: number[]; onset: number[]; centroid_hz: number[] };
  summary: { peak_dbfs: number; rms_dbfs: number; dynamic_contrast_db: number; rhythmic_density: number; mean_centroid_hz: number };
  warnings: string[];
}
export interface WaveformPeaks {
  sample_rate: number;
  samples_per_bucket: number;
  peaks: number[];
}
export interface Scene {
  index: number;
  start: Tick;
  end: Tick;
  motion: number;
  motion_peak: number;
  brightness: number;
  contrast: number;
  sharpness: number;
  stability: number;
  energy: number;
  quality: number;
  focus_x: number;
  focus_y: number;
  tags: string[];
  usable: boolean;
}
export interface VideoAnalysis {
  analysis_fps: Rational;
  frame_count: number;
  cuts: { t: Tick; kind: "hard" | "gradual" | "black"; score: number }[];
  scenes: Scene[];
}

// ---- system ----
export interface FfmpegTools {
  ffmpeg: string;
  ffprobe: string;
  version: string;
  source: string;
}
export interface AppInfo {
  version: string;
  ffmpeg?: FfmpegTools | null;
  ffmpeg_error?: string | null;
  data_dir: string;
  cache_dir: string;
  projects_dir: string;
  cpu_threads: number;
}
export interface EncoderInfo {
  name: string;
  codec: "h264" | "hevc";
  hardware: boolean;
  vendor: string;
  listed: boolean;
  available: boolean;
}
export type JobStatus = "queued" | "running" | "completed" | "failed" | "cancelled";
export interface JobInfo {
  id: string;
  kind: string;
  label: string;
  owner?: string | null;
  status: JobStatus;
  progress?: number | null;
  stage: string;
  error?: StudioError | null;
  result?: unknown;
  created_at: number;
  finished_at?: number | null;
  elapsed_ms?: number | null;
}

// ---- profiles ----
export interface ProfileDefaults {
  preset_id: string;
  variant: Variant;
  duration_secs: number;
  aspect: string;
  fps: Rational;
  platform: string;
  controls: CreativeControls;
  export_encoder: string;
  export_quality: number;
  export_dir?: string | null;
  media_folder?: string | null;
  naming: string;
}
export interface Profile {
  id: string;
  name: string;
  avatar: string;
  color: string;
  description: string;
  primary_niche?: string | null;
  niches: string[];
  platforms: string[];
  channel_urls: string[];
  branding?: string | null;
  defaults: ProfileDefaults;
  archived: boolean;
  created_at: number;
  updated_at: number;
}
export interface ProfileStats {
  projects: number;
  renders: number;
  assets: number;
}
export interface DeleteReport {
  project_entries_removed: number;
  render_entries_removed: number;
  asset_references_removed: number;
  orphaned_media_entries: number;
}
export interface ProfileAsset {
  fingerprint: string;
  path: string;
  kind: string;
  duration_s: number;
  shared: boolean;
  owner_profile: string;
}

// ---- clip discovery ----
export interface ClipQuery {
  text: string;
  tags: string[];
  min_motion?: number | null;
  max_motion?: number | null;
  min_quality?: number | null;
  min_duration_secs?: number | null;
  max_duration_secs?: number | null;
  sort: "energy" | "quality" | "motion" | "duration";
  limit: number;
}
export interface ClipHit {
  fingerprint: string;
  path: string;
  shared: boolean;
  scene_index: number;
  start: Tick;
  end: Tick;
  motion: number;
  energy: number;
  quality: number;
  tags: string[];
  relevance: number;
}
export interface DiscoveryResult {
  hits: ClipHit[];
  searched_videos: number;
  unanalyzed: string[];
  semantic_search_available: boolean;
}
