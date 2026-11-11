import { Lock, RefreshCcw, Replace, Sparkles, Unlock } from "lucide-react";
import { api } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import type { RegenMode } from "../types/studio";
import { ticksToSeconds } from "../lib/time";
import { IconButton } from "./ui";

/** Run a regeneration mode as a background job and resync the project. */
export async function runRegen(mode: RegenMode) {
  const st = useApp.getState();
  try {
    const id = await api.regenerateEdit(mode);
    const j = await waitForJob(id);
    await st.refreshProject();
    const r = j.result as { clips: number; warnings: string[] };
    st.toast({ kind: "success", title: `Done: ${r.clips} clips (previous version saved in History)`, details: r.warnings.join("\n") || undefined });
  } catch (e) {
    st.error(e, "Regeneration failed");
  }
}

/** Timeline toolbar group: lock / regenerate controls (V4 §14). */
export function RegenTools() {
  const { project, selectedClipId, range, editTimeline } = useApp();
  const clip = project?.timeline.tracks.flatMap((t) => t.clips).find((c) => c.id === selectedClipId);
  const hasEdit = !!project?.timeline.tracks.some((t) => t.kind === "video" && t.clips.length);
  return (
    <div className="flex items-center gap-0.5 border-l border-line pl-1.5">
      <IconButton title={clip?.locked ? "Unlock clip (L)" : "Lock clip (L)"} disabled={!clip} onClick={() => clip && editTimeline({ op: "set_clip_locked", clip_id: clip.id, locked: !clip.locked })}>
        {clip?.locked ? <Unlock size={14} /> : <Lock size={14} />}
      </IconButton>
      <IconButton title="Lock all clips inside the selected range (Shift+drag on the ruler)" disabled={!range} onClick={() => range && editTimeline({ op: "lock_range", from: range.start, to: range.end, exclusive: false })}>
        <Lock size={14} className="text-accent" />
      </IconButton>
      <IconButton title="Regenerate unlocked clips" disabled={!hasEdit} onClick={() => runRegen({ mode: "keep_locked" })}>
        <RefreshCcw size={14} />
      </IconButton>
      <IconButton
        title={range ? `Regenerate ${ticksToSeconds(range.start).toFixed(2)}–${ticksToSeconds(range.end).toFixed(2)}s only` : "Select a range first (Shift+drag on the ruler)"}
        disabled={!range || !hasEdit}
        onClick={() => range && runRegen({ mode: "range", start_secs: ticksToSeconds(range.start), end_secs: ticksToSeconds(range.end) })}
      >
        <RefreshCcw size={14} className="text-accent" />
      </IconButton>
      <IconButton title="Replace the selected clip's scene" disabled={!clip || clip.locked} onClick={() => clip && runRegen({ mode: "replace_clip", clip_id: clip.id })}>
        <Replace size={14} />
      </IconButton>
      <IconButton title="Reapply automatic effects (keep clips and timing)" disabled={!hasEdit} onClick={() => runRegen({ mode: "effects_only" })}>
        <Sparkles size={14} />
      </IconButton>
    </div>
  );
}
