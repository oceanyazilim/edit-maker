import { Crosshair, Plus, Trash2 } from "lucide-react";
import type { Cue, MediaAsset } from "../types/studio";
import { useApp } from "../stores/app";
import { ticksToSeconds } from "../lib/time";
import { IconButton, inputClass, Panel } from "./ui";

/**
 * Pinned moments: "show <clip> at <source second> exactly when the music reaches <second>".
 * With "on beat" the planner moves the cue to the nearest beat / kick / downbeat within half a beat.
 */
export function CuePanel({ cues, footage, onChange }: { cues: Cue[]; footage: MediaAsset[]; onChange: (c: Cue[]) => void }) {
  const playhead = useApp((s) => s.playhead);
  const set = (i: number, patch: Partial<Cue>) => onChange(cues.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  const num = (v: string) => (v.trim() === "" ? null : Number(v));
  return (
    <Panel
      title={`Cues (${cues.length})`}
      className="shrink-0"
      actions={
        <IconButton
          title="Add cue at the playhead"
          onClick={() => onChange([...cues, { at_secs: Math.round(ticksToSeconds(playhead) * 100) / 100, asset_id: footage[0]?.id ?? null, source_secs: null, snap_to_beat: true, label: "" }])}
        >
          <Plus size={14} />
        </IconButton>
      }
    >
      {cues.length === 0 && <p className="text-[11px] leading-relaxed text-dim">Pin a clip moment to a second of the song, e.g. the hook at 12 s. The rest of the edit is planned around it.</p>}
      <div className="flex flex-col gap-2">
        {cues.map((c, i) => (
          <div key={i} className="rounded-md border border-line bg-base p-2">
            <div className="flex items-center gap-1">
              <input className={inputClass + " h-7 w-16 text-xs"} type="number" step={0.01} min={0} value={c.at_secs} onChange={(e) => set(i, { at_secs: Math.max(0, Number(e.target.value) || 0) })} title="Output second" />
              <span className="text-[11px] text-dim">s</span>
              <IconButton title="Use playhead position" onClick={() => set(i, { at_secs: Math.round(ticksToSeconds(playhead) * 100) / 100 })}>
                <Crosshair size={13} />
              </IconButton>
              <label className="ml-auto flex items-center gap-1 text-[11px] text-muted" title="Move to the nearest beat / kick / downbeat within half a beat">
                <input type="checkbox" checked={c.snap_to_beat} onChange={(e) => set(i, { snap_to_beat: e.target.checked })} /> on beat
              </label>
              <IconButton title="Remove cue" onClick={() => onChange(cues.filter((_, j) => j !== i))}>
                <Trash2 size={12} />
              </IconButton>
            </div>
            <div className="mt-1.5 flex items-center gap-1">
              <select className={inputClass + " h-7 min-w-0 flex-1 text-xs"} value={c.asset_id ?? ""} onChange={(e) => set(i, { asset_id: e.target.value || null })}>
                <option value="">Any clip (planner decides)</option>
                {footage.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
              <input
                className={inputClass + " h-7 w-16 text-xs"}
                type="number"
                step={0.01}
                min={0}
                placeholder="src s"
                disabled={!c.asset_id}
                value={c.source_secs ?? ""}
                onChange={(e) => set(i, { source_secs: num(e.target.value) })}
                title="Moment inside the clip (seconds) that should appear at the cue"
              />
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
