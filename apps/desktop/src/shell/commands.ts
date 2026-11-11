// Editor commands shared by the menu bar, toolbars and keyboard shortcuts.
// Every command calls the real backend; availability checks live in `can`.
import { api, dialogs } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import { runRegen } from "../components/RegenTools";
import { openProjectFlow } from "../pages/Projects";
import type { Clip, Track } from "../types/studio";
import { secondsToTicks, ticksToSeconds, timecode } from "../lib/time";

const st = () => useApp.getState();

export const tlDuration = () => {
  const tl = st().project?.timeline;
  if (!tl) return 0;
  return tl.duration ?? Math.max(0, ...tl.tracks.flatMap((t) => t.clips.map((c) => c.start + c.duration)));
};

export function allClips(): { clip: Clip; track: Track }[] {
  const tl = st().project?.timeline;
  return tl ? tl.tracks.flatMap((track) => track.clips.map((clip) => ({ clip, track }))) : [];
}
export const selectedClip = () => allClips().find((x) => x.clip.id === st().selectedClipId) ?? null;

/** The clip a split should act on: the selection if it spans the playhead, else the top visible video clip there. */
function splitTarget(at: number) {
  const sel = selectedClip();
  if (sel && at > sel.clip.start && at < sel.clip.start + sel.clip.duration) return sel.clip;
  const tl = st().project?.timeline;
  for (const t of tl?.tracks.filter((t) => t.kind === "video" && !t.locked) ?? []) {
    const c = t.clips.find((c) => at > c.start && at < c.start + c.duration);
    if (c) return c;
  }
  return null;
}

export const can = {
  project: () => !!st().project,
  undo: () => st().canUndo,
  redo: () => st().canRedo,
  split: () => !!splitTarget(st().playhead),
  selection: () => !!selectedClip(),
  unlockedSelection: () => {
    const s = selectedClip();
    return !!s && !s.clip.locked && !s.track.locked;
  },
  edit: () => !!st().project?.timeline.tracks.some((t) => t.kind === "video" && t.clips.length),
  range: () => !!st().range,
  music: () => !!st().project?.assets.some((a) => a.role === "music"),
};

async function importWith(paths: string[], role: "music" | "footage" | "overlay") {
  if (!paths.length) return;
  try {
    const j = await waitForJob(await api.importMedia(paths, role));
    const r = j.result as { errors?: string[] } | null;
    if (r?.errors?.length) st().toast({ kind: "error", title: `${r.errors.length} file(s) could not be imported`, details: r.errors.join("\n") });
    else st().toast({ kind: "success", title: `Imported ${paths.length} file(s)` });
  } catch (e) {
    st().error(e, "Import failed");
  }
}

export const cmd = {
  newProject: () => st().setNewProjectOpen(true),
  openProject: () => openProjectFlow(),
  save: async () => {
    if (await st().saveProject()) st().toast({ kind: "success", title: "Project saved" });
  },
  closeProject: async () => {
    const s = st();
    if (!s.project) return;
    if (s.dirty && !window.confirm(`Close "${s.project.name}"? Unsaved changes are kept only in the autosave.`)) return;
    try {
      await api.closeProject();
      s.setProject(null);
      s.setRoute("home");
    } catch (e) {
      s.error(e, "Could not close the project");
    }
  },
  importFootage: async () => importWith(await dialogs.pickVideos(), "footage"),
  importMusic: async () => {
    const p = await dialogs.pickMusic();
    if (p) await importWith([p], "music");
  },
  importFolder: async () => {
    const dir = await dialogs.pickFolder();
    if (!dir) return;
    try {
      const j = await waitForJob(await api.importFolder(dir));
      const r = j.result as { imported: number; skipped: number; errors: string[] };
      st().toast({ kind: r.errors.length ? "error" : "success", title: `${r.imported} clip(s) imported, ${r.skipped} already in project`, details: r.errors.join("\n") || undefined });
    } catch (e) {
      st().error(e, "Folder import failed");
    }
  },
  export: () => st().project && st().setExportOpen(true),

  undo: () => st().editTimeline({ op: "undo" }),
  redo: () => st().editTimeline({ op: "redo" }),
  split: async (at = st().playhead) => {
    const c = splitTarget(at);
    if (c) await st().editTimeline({ op: "split", clip_id: c.id, at });
  },
  splitClipAt: (clipId: string, at: number) => st().editTimeline({ op: "split", clip_id: clipId, at }),
  deleteSelected: async (ripple = false) => {
    const s = selectedClip();
    if (!s) return;
    const ok = await st().editTimeline({ op: ripple ? "ripple_delete" : "delete", clip_id: s.clip.id });
    if (ok) st().selectClip(null);
  },
  toggleLock: () => {
    const s = selectedClip();
    if (s) void st().editTimeline({ op: "set_clip_locked", clip_id: s.clip.id, locked: !s.clip.locked });
  },
  lockRange: () => {
    const r = st().range;
    if (r) void st().editTimeline({ op: "lock_range", from: r.start, to: r.end, exclusive: false });
  },
  clearRange: () => st().setRange(null),
  resetFraming: () => {
    const s = selectedClip();
    if (s) void st().editTimeline({ op: "update_clip", clip: { ...s.clip, framing: { ...s.clip.framing, center_x: 0.5, center_y: 0.5, zoom: 1, rotation_deg: 0 }, keyframes: [] } });
  },
  clearEffects: () => {
    const s = selectedClip();
    if (s && s.clip.effects.length) void st().editTimeline({ op: "update_clip", clip: { ...s.clip, effects: [] } });
  },
  replaceScene: () => {
    const s = selectedClip();
    if (s && !s.clip.locked) void runRegen({ mode: "replace_clip", clip_id: s.clip.id });
  },

  addMarker: () => {
    const s = st();
    const fps = s.project?.timeline.settings.fps ?? { num: 30, den: 1 };
    void s.editTimeline({ op: "add_marker", t: s.playhead, label: `Marker ${timecode(s.playhead, fps)}` });
  },
  removeMarker: (id: string) => st().editTimeline({ op: "remove_marker", id }),
  addTrack: (kind: "video" | "audio") => {
    const n = (st().project?.timeline.tracks.filter((t) => t.kind === kind).length ?? 0) + 1;
    void st().editTimeline({ op: "add_track", kind, name: kind === "video" ? `Video ${n}` : `Audio ${n}` });
  },
  /** Insert an asset at the playhead on the first video track with room, else a new one. */
  insertAsset: async (assetId: string) => {
    const s = st();
    const p = s.project;
    if (!p) return;
    const asset = p.assets.find((a) => a.id === assetId);
    if (!asset) return;
    const isAudio = asset.kind === "audio";
    const at = s.playhead;
    const dur = Math.min(secondsToTicks(3), asset.kind === "image" ? secondsToTicks(3) : asset.metadata.duration);
    const tracks = p.timeline.tracks.filter((t) => t.kind === (isAudio ? "audio" : "video") && !t.locked);
    const free = tracks.find((t) => !t.clips.some((c) => c.start < at + dur && c.start + c.duration > at));
    if (!free) {
      s.toast({ kind: "error", title: "No free space at the playhead", details: "Move the playhead to an empty spot or add a track (Sequence → Add track)." });
      return;
    }
    const tl = await s.editTimeline({ op: "insert_clip", track_id: free.id, asset_id: assetId, start: at, duration: dur });
    if (tl) s.toast({ kind: "success", title: `Added "${asset.name}" to ${free.name} at ${ticksToSeconds(at).toFixed(2)}s` });
  },

  generate: async () => {
    const s = st();
    const p = s.project;
    if (!p) return;
    try {
      const id = await api.generateEdit();
      const j = await waitForJob(id);
      await s.refreshProject();
      const r = j.result as { clips: number; warnings: string[] };
      s.toast({ kind: "success", title: `Edit generated: ${r.clips} shots`, details: r.warnings.join("\n") || undefined });
    } catch (e) {
      s.error(e, "Generation failed");
    }
  },
  regenUnlocked: () => runRegen({ mode: "keep_locked" }),
  regenRange: () => {
    const r = st().range;
    if (r) void runRegen({ mode: "range", start_secs: ticksToSeconds(r.start), end_secs: ticksToSeconds(r.end) });
  },
  reapplyEffects: () => runRegen({ mode: "effects_only" }),
  renderPreview: () => window.dispatchEvent(new CustomEvent("studio:render-preview")),

  zoomIn: () => st().setTlZoom(st().tlZoom * 1.25),
  zoomOut: () => st().setTlZoom(st().tlZoom / 1.25),
  zoomFit: () => window.dispatchEvent(new CustomEvent("studio:timeline-fit")),
};
