import { create } from "zustand";
import type { JobInfo, Profile, Project, StudioError, Tick, Timeline } from "../types/studio";
import { api, toStudioError, type TimelineOp } from "../services/api";

export type Route =
  | "home"
  | "auto"
  | "editor"
  | "audio"
  | "library"
  | "discover"
  | "niches"
  | "presets"
  | "export"
  | "projects"
  | "settings"
  | "profiles"
  | "trends"
  | "batch"
  | "campaigns";

/** Left tool-rail panels. */
export type ToolId = "media" | "auto" | "text" | "transitions" | "effects" | "overlays" | "audio" | "captions" | "library" | "presets";
export interface Layout {
  left: boolean;
  right: boolean;
  bottom: boolean;
}

function readLS<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? ({ ...fallback, ...JSON.parse(v) } as T) : fallback;
  } catch {
    return fallback;
  }
}
function readLSString<T extends string>(key: string, fallback: T, allowed: readonly T[]): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : fallback;
  } catch {
    return fallback;
  }
}
function writeLS(key: string, v: unknown) {
  try {
    localStorage.setItem(key, typeof v === "string" ? v : JSON.stringify(v));
  } catch {
    /* storage unavailable: layout simply isn't remembered */
  }
}
const TOOLS: readonly ToolId[] = ["media", "auto", "text", "transitions", "effects", "overlays", "audio", "captions", "library", "presets"];

export interface Toast {
  id: number;
  kind: "info" | "error" | "success";
  title: string;
  details?: string;
}

interface AppStore {
  route: Route;
  setRoute: (r: Route) => void;

  profile: Profile | null;
  setProfile: (p: Profile | null) => void;
  profileDialog: { open: boolean; edit?: Profile | null };
  setProfileDialog: (v: { open: boolean; edit?: Profile | null }) => void;

  project: Project | null;
  setProject: (p: Project | null) => void;
  /** Replace the project after an in-place change (settings, assets) keeping playhead/selection. */
  patchProject: (p: Project) => void;
  refreshProject: () => Promise<void>;

  canUndo: boolean;
  canRedo: boolean;
  undoLabel?: string | null;
  redoLabel?: string | null;
  editTimeline: (op: TimelineOp) => Promise<Timeline | null>;

  playhead: Tick;
  setPlayhead: (t: Tick) => void;
  selectedClipId: string | null;
  selectClip: (id: string | null) => void;
  /** Timeline range selection (ticks) used by "regenerate range" and "lock range". */
  range: { start: Tick; end: Tick } | null;
  setRange: (r: { start: Tick; end: Tick } | null) => void;

  jobs: Record<string, JobInfo>;
  upsertJob: (j: JobInfo) => void;

  toasts: Toast[];
  toast: (t: Omit<Toast, "id">) => void;
  error: (e: unknown, title?: string) => void;
  dismiss: (id: number) => void;

  newProjectOpen: boolean;
  setNewProjectOpen: (v: boolean) => void;

  /** Workstation layout (persisted per machine). */
  layout: Layout;
  toggleLayout: (k: keyof Layout, v?: boolean) => void;
  leftTool: ToolId;
  setLeftTool: (t: ToolId) => void;
  /** Timeline zoom in pixels per second, tool and snapping (shared by toolbar and menus). */
  tlZoom: number;
  setTlZoom: (pps: number) => void;
  tlTool: "select" | "razor";
  setTlTool: (t: "select" | "razor") => void;
  snapOn: boolean;
  setSnapOn: (v: boolean) => void;
  exportOpen: boolean;
  setExportOpen: (v: boolean) => void;
  jobsOpen: boolean;
  setJobsOpen: (v: boolean) => void;
  /** Unsaved changes since the last explicit save (autosave still protects them). */
  dirty: boolean;
  setDirty: (v: boolean) => void;
  lastSavedAt: number | null;
  saveProject: () => Promise<boolean>;
}

let toastId = 1;

export const useApp = create<AppStore>((set, get) => ({
  route: "home",
  // "auto" is the workstation with the Auto Edit panel open.
  setRoute: (route) => {
    if (route === "auto") {
      writeLS("ws:tool", "auto");
      set({ route: "editor", leftTool: "auto", layout: { ...get().layout, left: true } });
    } else set({ route });
  },

  profile: null,
  setProfile: (profile) => set({ profile }),
  profileDialog: { open: false },
  setProfileDialog: (profileDialog) => set({ profileDialog }),

  project: null,
  setProject: (project) => set({ project, selectedClipId: null, playhead: 0, range: null, dirty: false }),
  patchProject: (project) => {
    const sel = get().selectedClipId;
    const exists = sel && project.timeline.tracks.some((t) => t.clips.some((c) => c.id === sel));
    set({ project, dirty: true, selectedClipId: exists ? sel : null });
  },
  refreshProject: async () => {
    try {
      const p = await api.getProject();
      const prev = get().project;
      set({ project: p, dirty: prev && p && prev.id === p.id ? get().dirty || prev.modified_at !== p.modified_at : false });
    } catch (e) {
      get().error(e);
    }
  },

  canUndo: false,
  canRedo: false,
  editTimeline: async (op) => {
    try {
      const s = await api.timelineEdit(op);
      const p = get().project;
      set({
        project: p ? { ...p, timeline: s.timeline } : p,
        canUndo: s.can_undo,
        canRedo: s.can_redo,
        undoLabel: s.undo_label,
        redoLabel: s.redo_label,
        dirty: true,
      });
      return s.timeline;
    } catch (e) {
      get().error(e, "Edit rejected");
      return null;
    }
  },

  playhead: 0,
  setPlayhead: (playhead) => set({ playhead: Math.max(0, playhead) }),
  selectedClipId: null,
  selectClip: (selectedClipId) => set({ selectedClipId }),
  range: null,
  setRange: (range) => set({ range }),

  jobs: {},
  upsertJob: (j) => set((s) => ({ jobs: { ...s.jobs, [j.id]: j } })),

  toasts: [],
  toast: (t) => {
    const id = toastId++;
    set((s) => ({ toasts: [...s.toasts.slice(-4), { ...t, id }] }));
    setTimeout(() => get().dismiss(id), t.kind === "error" ? 9000 : 4000);
  },
  error: (e, title) => {
    const err: StudioError = toStudioError(e);
    get().toast({ kind: "error", title: title ? `${title}: ${err.message}` : err.message, details: err.details });
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  newProjectOpen: false,
  setNewProjectOpen: (newProjectOpen) => set({ newProjectOpen }),

  layout: readLS<Layout>("ws:layout", { left: true, right: true, bottom: true }),
  toggleLayout: (k, v) => {
    const layout = { ...get().layout, [k]: v ?? !get().layout[k] };
    writeLS("ws:layout", layout);
    set({ layout });
  },
  leftTool: readLSString<ToolId>("ws:tool", "auto", TOOLS),
  setLeftTool: (leftTool) => {
    writeLS("ws:tool", leftTool);
    set({ leftTool, layout: { ...get().layout, left: true } });
  },
  tlZoom: 80,
  setTlZoom: (pps) => set({ tlZoom: Math.max(4, Math.min(800, pps)) }),
  tlTool: "select",
  setTlTool: (tlTool) => set({ tlTool }),
  snapOn: true,
  setSnapOn: (snapOn) => set({ snapOn }),
  exportOpen: false,
  setExportOpen: (exportOpen) => set({ exportOpen }),
  jobsOpen: false,
  setJobsOpen: (jobsOpen) => set({ jobsOpen }),
  dirty: false,
  setDirty: (dirty) => set({ dirty }),
  lastSavedAt: null,
  saveProject: async () => {
    if (!get().project) return false;
    try {
      await api.saveProject();
      set({ dirty: false, lastSavedAt: Date.now() });
      return true;
    } catch (e) {
      get().error(e, "Save failed");
      return false;
    }
  },
}));

/** Resolve when a job finishes; rejects on failure. */
export function waitForJob(id: string): Promise<JobInfo> {
  return new Promise((resolve, reject) => {
    const check = (j?: JobInfo) => {
      if (!j) return false;
      if (j.status === "completed") {
        resolve(j);
        return true;
      }
      if (j.status === "failed" || j.status === "cancelled") {
        reject(j.error ?? { code: j.status, message: j.status === "cancelled" ? "Cancelled." : "The job failed." });
        return true;
      }
      return false;
    };
    if (check(useApp.getState().jobs[id])) return;
    const unsub = useApp.subscribe((s) => {
      if (check(s.jobs[id])) unsub();
    });
  });
}
