import { useQueryClient } from "@tanstack/react-query";
import { Plus, Trash2, Undo2 } from "lucide-react";
import { useState } from "react";
import { api, waitForJobSafe } from "../services/api";
import { useApp } from "../stores/app";
import type { AudioAnalysis, AudioEdits } from "../types/studio";
import { Button, IconButton, inputClass, Panel, Slider } from "./ui";

/**
 * Persistent manual corrections (V4 §6). Stored in the project settings and re-applied to
 * every analysis, so they survive re-analysis and reopening the project.
 */
export function AudioCorrections({ analysis, compact = false }: { analysis: AudioAnalysis; compact?: boolean }) {
  const { project, patchProject: setProject, error } = useApp();
  const qc = useQueryClient();
  const [bpm, setBpm] = useState<string>(project?.settings.audio_edits.bpm_override?.toString() ?? "");
  const [dropAt, setDropAt] = useState("");
  if (!project) return null;
  const edits = project.settings.audio_edits;
  const sr = analysis.sample_rate;

  const save = async (next: AudioEdits, reanalyze = false) => {
    try {
      setProject(await api.updateSettings({ ...project.settings, audio_edits: next }));
      if (reanalyze) await waitForJobSafe(await api.analyzeAudio());
      await qc.invalidateQueries({ queryKey: ["audio-analysis"] });
      await qc.invalidateQueries({ queryKey: ["segments"] });
    } catch (e) {
      error(e, "Correction not applied");
    }
  };

  const drops = analysis.drops.filter((d) => d.kind !== "false_drop_candidate");
  return (
    <Panel title="Manual corrections (saved with the project)" className="shrink-0">
      <div className={compact ? "flex flex-col gap-4 text-xs" : "grid grid-cols-3 gap-5 text-xs"}>
        <div className="flex flex-col gap-2">
          <span className="text-[11px] text-muted">Tempo</span>
          <div className="flex gap-1">
            <input className={inputClass + " w-24"} type="number" step={0.01} min={40} max={300} placeholder={analysis.tempo.bpm.toFixed(2)} value={bpm} onChange={(e) => setBpm(e.target.value)} />
            <Button size="sm" onClick={() => save({ ...edits, bpm_override: bpm.trim() === "" ? null : Number(bpm) }, true)}>
              Apply
            </Button>
            {edits.bpm_override != null && (
              <IconButton title="Back to detected tempo" onClick={() => { setBpm(""); void save({ ...edits, bpm_override: null }, true); }}>
                <Undo2 size={13} />
              </IconButton>
            )}
          </div>
          <div className="flex gap-1">
            {analysis.tempo.alternatives.slice(0, 2).map((a) => (
              <button key={a.bpm} className="rounded bg-raised px-1.5 py-0.5 text-[10px] text-muted hover:text-fg" title="Half/double-time suggestion" onClick={() => { setBpm(a.bpm.toFixed(2)); }}>
                {a.bpm.toFixed(1)}?
              </button>
            ))}
          </div>
        </div>
        <div className="flex flex-col gap-2">
          <Slider label="Beat grid offset" min={-250} max={250} step={1} value={edits.grid_offset_ms} format={(v) => `${v.toFixed(0)} ms`} onChange={(v) => setProject({ ...project, settings: { ...project.settings, audio_edits: { ...edits, grid_offset_ms: v } } })} />
          <Button size="sm" variant="ghost" onClick={() => save(edits)}>
            Save grid offset
          </Button>
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] text-muted">Drops</span>
          {drops.map((d) => {
            const t = d.sample / sr;
            const manual = edits.added_drops_secs.some((x) => Math.abs(x - t) < 0.3);
            return (
              <div key={d.sample} className="flex items-center justify-between rounded border border-line px-1.5 py-0.5">
                <span className="tabular text-[11px]">
                  {t.toFixed(2)}s · {d.kind.replace(/_/g, " ")} {manual && <span className="text-accent">(manual)</span>}
                </span>
                <IconButton
                  title="Remove this drop"
                  onClick={() =>
                    save(
                      manual
                        ? { ...edits, added_drops_secs: edits.added_drops_secs.filter((x) => Math.abs(x - t) >= 0.3) }
                        : { ...edits, removed_drops_secs: [...edits.removed_drops_secs, t] },
                    )
                  }
                >
                  <Trash2 size={12} />
                </IconButton>
              </div>
            );
          })}
          <div className="flex gap-1">
            <input className={inputClass + " h-7 w-20 text-xs"} type="number" step={0.01} min={0} placeholder="sec" value={dropAt} onChange={(e) => setDropAt(e.target.value)} />
            <Button size="sm" icon={<Plus size={12} />} disabled={dropAt.trim() === ""} onClick={() => { void save({ ...edits, added_drops_secs: [...edits.added_drops_secs, Number(dropAt)] }); setDropAt(""); }}>
              Add drop
            </Button>
          </div>
          {edits.removed_drops_secs.length > 0 && (
            <button className="self-start text-[10px] text-dim hover:text-fg" onClick={() => save({ ...edits, removed_drops_secs: [] })}>
              Restore {edits.removed_drops_secs.length} removed detected drop(s)
            </button>
          )}
        </div>
      </div>
    </Panel>
  );
}
