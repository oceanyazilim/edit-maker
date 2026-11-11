import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { Badge, Button, inputClass, Panel, cx } from "../components/ui";

export default function NicheExplorer() {
  const niches = useQuery({ queryKey: ["niches"], queryFn: api.listNiches });
  const { project, patchProject: setProject, error, toast } = useApp();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<string | null>(null);
  const list = useMemo(() => (niches.data ?? []).filter((n) => `${n.name} ${n.category}`.toLowerCase().includes(q.toLowerCase())), [niches.data, q]);
  const n = list.find((x) => x.id === sel) ?? list[0];
  const cats = Array.from(new Set(list.map((x) => x.category)));
  const apply = async () => {
    if (!project || !n) return;
    try {
      setProject(await api.updateSettings({ ...project.settings, niche_id: n.id, preset_id: n.default_preset }));
      toast({ kind: "success", title: `Project niche set to ${n.name}` });
    } catch (e) {
      error(e);
    }
  };
  return (
    <div className="flex h-full gap-3 p-3">
      <Panel title="Niche profiles" className="w-80 shrink-0" bodyClass="overflow-y-auto p-2">
        <input className={inputClass + " mb-2 w-full"} placeholder="Search niches…" value={q} onChange={(e) => setQ(e.target.value)} />
        {cats.map((c) => (
          <div key={c} className="mb-2">
            <div className="px-1 text-[10px] font-semibold uppercase tracking-widest text-dim">{c}</div>
            {list.filter((x) => x.category === c).map((x) => (
              <button key={x.id} onClick={() => setSel(x.id)} className={cx("block w-full rounded px-2 py-1 text-left text-xs", n?.id === x.id ? "bg-raised" : "hover:bg-raised/60")}>{x.name}</button>
            ))}
          </div>
        ))}
      </Panel>
      {n && (
        <Panel title={n.name} className="flex-1" actions={project && <Button size="sm" variant="primary" onClick={apply}>Use for current project</Button>}>
          <p className="text-xs text-muted">{n.description}</p>
          <div className="mt-4 grid grid-cols-2 gap-6 text-xs">
            <div>
              <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-dim">Scene tag preferences</h4>
              {Object.entries(n.tag_weights).map(([k, v]) => (
                <div key={k} className="flex items-center gap-2 py-0.5">
                  <span className="w-28 text-muted">{k}</span>
                  <div className="h-1.5 flex-1 rounded bg-raised"><div className={cx("h-full rounded", v >= 0 ? "bg-ok/70" : "bg-err/70")} style={{ width: `${Math.abs(v) * 100}%` }} /></div>
                  <span className="tabular w-10 text-right">{v.toFixed(1)}</span>
                </div>
              ))}
            </div>
            <div className="flex flex-col gap-1.5">
              <div className="flex justify-between"><span className="text-muted">Default preset</span><Badge tone="accent">{n.default_preset}</Badge></div>
              <div className="flex justify-between"><span className="text-muted">Cut density bias</span><span>{n.cut_density_bias.toFixed(2)}</span></div>
              <div className="flex justify-between"><span className="text-muted">Effect intensity bias</span><span>{n.effect_intensity_bias.toFixed(2)}</span></div>
              <div className="flex justify-between"><span className="text-muted">Transition bias</span><span>{n.transition_bias.toFixed(2)}</span></div>
              <div className="flex justify-between"><span className="text-muted">Energy bias</span><span>{n.energy_bias.toFixed(2)}</span></div>
              <div className="flex justify-between"><span className="text-muted">Formats</span><span>{n.recommended_aspects.join(", ")} · {n.recommended_duration_secs}s</span></div>
              <div className="flex justify-between"><span className="text-muted">Typical music</span><span>{n.suggested_music.join(", ")}</span></div>
              <p className="mt-2 text-[11px] text-dim">Profiles are adjustable starting points, not universal rules. Tags refer to measured scene properties.</p>
            </div>
          </div>
        </Panel>
      )}
    </div>
  );
}
