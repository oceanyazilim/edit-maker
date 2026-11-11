import { useMutation } from "@tanstack/react-query";
import { Info, ScanSearch, Share2 } from "lucide-react";
import { useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import type { ClipQuery } from "../types/studio";
import { ticksToSeconds } from "../lib/time";
import { Badge, Button, EmptyState, Field, inputClass, Panel, Select, Slider } from "../components/ui";

const TAGS = ["high_motion", "low_motion", "action_peak", "dark", "bright", "colorful", "monochrome", "pan_left", "pan_right", "tilt_up", "tilt_down", "static_camera", "shaky", "long_take"];

export default function ClipDiscovery() {
  const { profile, error, toast } = useApp();
  const [q, setQ] = useState<ClipQuery>({ text: "", tags: [], sort: "energy", limit: 200, min_motion: 0, max_motion: 1, min_quality: 0 });
  const search = useMutation({ mutationFn: api.discoverClips, onError: (e) => error(e, "Search failed") });
  const r = search.data;
  const toggle = (t: string) => setQ({ ...q, tags: q.tags.includes(t) ? q.tags.filter((x) => x !== t) : [...q.tags, t] });

  return (
    <div className="flex h-full gap-3 p-3">
      <Panel title={`Find clips — ${profile?.name ?? ""}`} className="w-80 shrink-0">
        <div className="flex flex-col gap-3">
          <Field label="Text (file name or tag)">
            <input className={inputClass} value={q.text} onChange={(e) => setQ({ ...q, text: e.target.value })} onKeyDown={(e) => e.key === "Enter" && search.mutate(q)} placeholder="e.g. drift, night" />
          </Field>
          <Field label="Required tags">
            <div className="flex flex-wrap gap-1">
              {TAGS.map((t) => (
                <button key={t} onClick={() => toggle(t)} className={`rounded px-1.5 py-0.5 text-[11px] ${q.tags.includes(t) ? "bg-accent text-white" : "bg-raised text-muted"}`}>{t}</button>
              ))}
            </div>
          </Field>
          <Slider label="Min motion" value={q.min_motion ?? 0} onChange={(v) => setQ({ ...q, min_motion: v })} />
          <Slider label="Max motion" value={q.max_motion ?? 1} onChange={(v) => setQ({ ...q, max_motion: v })} />
          <Slider label="Min quality" value={q.min_quality ?? 0} onChange={(v) => setQ({ ...q, min_quality: v })} />
          <div className="grid grid-cols-2 gap-2">
            <Field label="Min duration (s)">
              <input className={inputClass} type="number" min={0} value={q.min_duration_secs ?? ""} onChange={(e) => setQ({ ...q, min_duration_secs: e.target.value === "" ? null : Number(e.target.value) })} />
            </Field>
            <Field label="Max duration (s)">
              <input className={inputClass} type="number" min={0} value={q.max_duration_secs ?? ""} onChange={(e) => setQ({ ...q, max_duration_secs: e.target.value === "" ? null : Number(e.target.value) })} />
            </Field>
          </div>
          <Field label="Sort by">
            <Select value={q.sort} onChange={(v) => setQ({ ...q, sort: v })} options={[{ value: "energy", label: "Visual energy" }, { value: "motion", label: "Motion" }, { value: "quality", label: "Quality" }, { value: "duration", label: "Duration" }]} />
          </Field>
          <Button variant="primary" icon={<ScanSearch size={14} />} disabled={search.isPending} onClick={() => search.mutate(q)}>Search library</Button>
          <p className="flex gap-1.5 text-[11px] leading-relaxed text-dim">
            <Info size={13} className="mt-0.5 shrink-0" />
            Searches this profile's media and media other profiles shared, using cached scene analyses. Character / subject recognition is not available yet — text only matches file names and measured tags. External providers are not connected.
          </p>
        </div>
      </Panel>
      <Panel title={r ? `${r.hits.length} scenes in ${r.searched_videos} analyzed videos` : "Results"} className="min-w-0 flex-1" bodyClass="overflow-auto p-0">
        {!r ? (
          <EmptyState icon={<ScanSearch size={24} />} title="Search your indexed library" body="Import and analyze footage first; results come from the scene database." />
        ) : (
          <>
            {r.unanalyzed.length > 0 && <div className="border-b border-line px-3 py-2 text-[11px] text-warn">{r.unanalyzed.length} video(s) are not analyzed yet and were skipped (Media Library → Analyze scenes).</div>}
            <table className="w-full text-xs">
              <thead className="text-left text-dim">
                <tr>
                  <th className="px-3 py-1.5 font-medium">File</th>
                  <th className="font-medium">Scene</th>
                  <th className="font-medium">Motion</th>
                  <th className="font-medium">Energy</th>
                  <th className="font-medium">Quality</th>
                  <th className="font-medium">Tags</th>
                </tr>
              </thead>
              <tbody>
                {r.hits.map((h) => (
                  <tr key={`${h.fingerprint}-${h.scene_index}`} className="cv-auto border-t border-line">
                    <td className="max-w-xs truncate px-3 py-1.5" title={h.path}>{h.path.split(/[\\/]/).pop()} {h.shared && <Badge tone="accent">shared</Badge>}</td>
                    <td className="tabular text-dim">#{h.scene_index} {ticksToSeconds(h.start).toFixed(1)}–{ticksToSeconds(h.end).toFixed(1)}s</td>
                    <td className="tabular">{h.motion.toFixed(2)}</td>
                    <td className="tabular">{h.energy.toFixed(2)}</td>
                    <td className="tabular">{h.quality.toFixed(2)}</td>
                    <td className="text-dim">{h.tags.join(", ")}</td>
                    <td className="px-2">
                      {!h.shared && (
                        <button title="Share with all profiles" className="text-dim hover:text-fg" onClick={async () => { try { await api.setAssetShared(h.fingerprint, true); toast({ kind: "success", title: "Media shared with all profiles" }); search.mutate(q); } catch (e) { error(e); } }}>
                          <Share2 size={13} />
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </Panel>
    </div>
  );
}
