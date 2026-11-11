import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, Save } from "lucide-react";
import { useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import type { EditPreset } from "../types/studio";
import { Badge, Button, EmptyState, Panel, cx } from "../components/ui";

export default function Presets() {
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  const qc = useQueryClient();
  const { toast, error } = useApp();
  const [sel, setSel] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const p = presets.data?.find((x) => x.id === sel) ?? presets.data?.[0];

  const duplicate = (src: EditPreset) => {
    const copy = { ...src, id: `${src.id}-custom`, name: `${src.name} (Custom)`, builtin: false, version: 1 };
    setDraft(JSON.stringify(copy, null, 2));
  };
  const save = async () => {
    if (!draft) return;
    try {
      const parsed = JSON.parse(draft) as EditPreset;
      const path = await api.savePreset(parsed);
      toast({ kind: "success", title: "Preset saved", details: path });
      setDraft(null);
      await qc.invalidateQueries({ queryKey: ["presets"] });
      setSel(parsed.id);
    } catch (e) {
      error(e instanceof SyntaxError ? { message: `Invalid JSON: ${e.message}` } : e, "Preset not saved");
    }
  };

  return (
    <div className="flex h-full gap-3 p-3">
      <Panel title={`Editing presets (${presets.data?.length ?? 0})`} className="w-80 shrink-0" bodyClass="overflow-y-auto p-1">
        {presets.data?.map((x) => (
          <button key={x.id} onClick={() => { setSel(x.id); setDraft(null); }} className={cx("flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-xs", p?.id === x.id ? "bg-raised" : "hover:bg-raised/60")}>
            <span className="truncate">{x.name}</span>
            {x.builtin ? <Badge>built-in</Badge> : <Badge tone="accent">custom</Badge>}
          </button>
        ))}
      </Panel>
      {!p ? (
        <EmptyState title="No presets" />
      ) : draft != null ? (
        <Panel title="Edit preset JSON" className="flex-1" actions={<><Button size="sm" variant="ghost" onClick={() => setDraft(null)}>Cancel</Button><Button size="sm" variant="primary" icon={<Save size={13} />} onClick={save}>Save preset</Button></>} bodyClass="p-0 h-full">
          <textarea className="h-full w-full resize-none bg-base p-3 font-mono text-[12px] text-fg outline-none" spellCheck={false} value={draft} onChange={(e) => setDraft(e.target.value)} />
        </Panel>
      ) : (
        <Panel title={p.name} className="flex-1" actions={<Button size="sm" icon={<Copy size={13} />} onClick={() => duplicate(p)}>Duplicate & edit</Button>}>
          <p className="mb-4 text-xs text-muted">{p.description}</p>
          <div className="grid grid-cols-3 gap-4 text-xs">
            <Group title="Pacing" rows={[["Base shot", `${p.pacing.base_shot_beats} beats`], ["Min / max shot", `${p.pacing.min_shot_secs}s / ${p.pacing.max_shot_secs}s`], ["Build-up acceleration", p.pacing.build_up_acceleration], ["Beat sync", p.beat_sync], ...Object.entries(p.pacing.section_multipliers).map(([k, v]) => [`× ${k.replace("_", " ")}`, v] as [string, unknown])]} />
            <Group title="Drop" rows={[["Impact", p.drop.impact], ["Position in output", `${Math.round(p.drop.position * 100)}%`], ["Flash", p.drop.flash], ["Zoom punch", p.drop.zoom_punch], ["RGB split", p.drop.rgb_split], ["Shake", p.drop.shake], ["Anticipation hold", `${p.drop.pre_drop_hold_beats} beats`], ["Pre-drop slow-mo", p.drop.pre_drop_slowmo < 1 ? `${p.drop.pre_drop_slowmo}×` : "off"], ["Velocity into drop", p.drop.velocity || "off"], ["Drop glitch / zoom blur", `${p.drop.glitch} / ${p.drop.zoom_blur}`]]} />
            <Group title="Effects & transitions" rows={[["Intensity", p.effects.intensity], ["Kick zoom / shake", `${p.effects.kick_zoom} / ${p.effects.kick_shake}`], ["Kick sections", p.effects.kick_sections.join(", ") || "—"], ["Cooldown", `${p.effects.cooldown_beats} beats`], ["Max per bar", p.effects.max_per_bar], ["Grade (b/c/s)", `${p.effects.color.brightness} / ${p.effects.color.contrast} / ${p.effects.color.saturation}`], ["Vignette", p.effects.vignette], ["Transitions", `${Math.round(p.transitions.frequency * 100)}% · ${p.transitions.kinds.join(", ")}`]]} />
          </div>
        </Panel>
      )}
    </div>
  );
}

function Group({ title, rows }: { title: string; rows: [string, unknown][] }) {
  return (
    <div>
      <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-dim">{title}</h4>
      {rows.map(([k, v]) => (
        <div key={k} className="flex justify-between border-b border-line py-1">
          <span className="text-muted">{k}</span>
          <span className="tabular">{String(v)}</span>
        </div>
      ))}
    </div>
  );
}
