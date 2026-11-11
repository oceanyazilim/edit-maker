import { useQuery } from "@tanstack/react-query";
import { Check } from "lucide-react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { Badge, Button, PanelHeader, Progress, cx } from "../components/ui";

/** Preset details from the preset engine; selecting one sets the project's style. */
export function PresetsPanel() {
  const project = useApp((s) => s.project);
  const patchProject = useApp((s) => s.patchProject);
  const error = useApp((s) => s.error);
  const setRoute = useApp((s) => s.setRoute);
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  if (!project) return null;
  const current = project.settings.preset_id;
  const use = async (id: string) => {
    try {
      patchProject(await api.updateSettings({ ...project.settings, preset_id: id }));
    } catch (e) {
      error(e);
    }
  };
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Presets" actions={<Button size="sm" variant="ghost" onClick={() => setRoute("presets")}>Edit presets</Button>} />
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {presets.isLoading && <Progress value={null} />}
        <div className="flex flex-col gap-1.5">
          {(presets.data ?? []).map((p) => (
            <button key={p.id} onClick={() => use(p.id)} className={cx("rounded-md border p-2 text-left", p.id === current ? "border-accent bg-accent/10" : "border-line bg-raised hover:border-line-strong")}>
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-[11px] font-medium">{p.name}</span>
                <span className="flex items-center gap-1">
                  {!p.builtin && <Badge tone="accent">mine</Badge>}
                  {p.id === current && <Check size={12} className="text-accent" />}
                </span>
              </div>
              <div className="mt-0.5 line-clamp-2 text-[10px] text-dim">{p.description}</div>
              <div className="tabular mt-1.5 grid grid-cols-3 gap-1 text-[9px] text-muted">
                <span>shot {p.pacing.base_shot_beats} beats</span>
                <span>fx {(p.effects.intensity * 100).toFixed(0)}%</span>
                <span>drop {(p.drop.impact * 100).toFixed(0)}%</span>
                <span>sync {(p.beat_sync * 100).toFixed(0)}%</span>
                <span>trans {(p.transitions.frequency * 100).toFixed(0)}%</span>
                <span>{p.output.aspect}</span>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
