import { useQuery } from "@tanstack/react-query";
import { Music2 } from "lucide-react";
import { api } from "../services/api";
import type { ProjectSettings } from "../types/studio";
import { Badge, cx, Panel } from "./ui";

const DURATIONS = [10, 15, 20, 30];
const DROP_POS: { value: string; label: string }[] = [
  { value: "", label: "Preset default" },
  { value: "0.15", label: "Early (15%)" },
  { value: "0.3", label: "After intro (30%)" },
  { value: "0.5", label: "Middle (50%)" },
  { value: "0.7", label: "Late (70%)" },
];

/**
 * Smart Music Segment Selector: ranked ranges built from the detected structure.
 * Choosing one fixes the music window; "Automatic" lets the planner choose.
 */
export function SegmentPanel({ settings, hasMusic, analyzed, onChange }: { settings: ProjectSettings; hasMusic: boolean; analyzed: boolean; onChange: (s: ProjectSettings) => void }) {
  const dur = settings.duration_secs;
  const q = useQuery({ queryKey: ["segments", dur, settings.audio_edits], queryFn: () => api.suggestSegments(dur), enabled: hasMusic && analyzed });
  const selected = settings.audio_start_secs;
  return (
    <Panel title="Music segment" className="shrink-0">
      <div className="mb-2 flex gap-1">
        {(DURATIONS.includes(dur) ? DURATIONS : [...DURATIONS, dur]).map((d) => (
          <button key={d} onClick={() => onChange({ ...settings, duration_secs: d, audio_start_secs: null })} className={cx("flex-1 rounded px-1.5 py-1 text-[11px]", Math.abs(dur - d) < 0.01 ? "bg-accent text-white" : "bg-raised text-muted hover:text-fg")}>
            {Number.isInteger(d) ? d : d.toFixed(2)}s
          </button>
        ))}
      </div>
      {!hasMusic ? (
        <p className="text-[11px] text-dim">Import music to get segment suggestions.</p>
      ) : !analyzed ? (
        <p className="text-[11px] text-dim">Analyze the music (Audio page) to get suggestions; generation will pick a range automatically otherwise.</p>
      ) : (
        <div className="flex flex-col gap-1">
          <button onClick={() => onChange({ ...settings, audio_start_secs: null })} className={cx("rounded-md border px-2 py-1.5 text-left text-[11px]", selected == null ? "border-accent/60 bg-accent/5" : "border-line hover:bg-raised")}>
            Automatic (planner chooses around the main drop)
          </button>
          {(q.data ?? []).map((s) => (
            <button
              key={`${s.kind}-${s.start_secs}`}
              title={s.reasons.join("\n")}
              onClick={() => onChange({ ...settings, audio_start_secs: s.start_secs })}
              className={cx("rounded-md border px-2 py-1.5 text-left", selected != null && Math.abs(selected - s.start_secs) < 0.01 ? "border-accent/60 bg-accent/5" : "border-line hover:bg-raised")}
            >
              <div className="flex items-center justify-between text-[11px]">
                <span className="flex items-center gap-1">
                  <Music2 size={11} className="text-dim" />
                  {s.label}
                </span>
                <Badge>{s.score.toFixed(2)}</Badge>
              </div>
              <div className="tabular mt-0.5 flex items-center justify-between text-[10px] text-dim">
                <span>
                  {s.start_secs.toFixed(2)}–{s.end_secs.toFixed(2)}s{s.drop_offset_secs != null && ` · drop at +${s.drop_offset_secs.toFixed(2)}s`}
                </span>
                {s.phrase_fit_duration_secs != null && (
                  <span
                    role="button"
                    title="Use the nearest length that ends on a musical phrase boundary"
                    className="rounded bg-raised px-1 text-muted hover:text-fg"
                    onClick={(e) => {
                      e.stopPropagation();
                      onChange({ ...settings, audio_start_secs: s.start_secs, duration_secs: Math.round(s.phrase_fit_duration_secs! * 100) / 100 });
                    }}
                  >
                    fit phrase {s.phrase_fit_duration_secs.toFixed(2)}s
                  </span>
                )}
              </div>
            </button>
          ))}
          {q.data?.length === 0 && <p className="text-[11px] text-dim">No structural suggestions for this length.</p>}
        </div>
      )}
      <label className="mt-2 flex items-center justify-between gap-2 text-[11px] text-muted">
        Drop placement
        <select
          className="h-6 rounded border border-line bg-base px-1 text-[11px]"
          value={settings.drop_position == null ? "" : String(settings.drop_position)}
          onChange={(e) => onChange({ ...settings, drop_position: e.target.value === "" ? null : Number(e.target.value) })}
          title="Used when the segment is chosen automatically"
        >
          {DROP_POS.map((d) => (
            <option key={d.value} value={d.value}>
              {d.label}
            </option>
          ))}
        </select>
      </label>
    </Panel>
  );
}
