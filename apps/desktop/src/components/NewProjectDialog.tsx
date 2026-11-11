import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import type { CreativeControls, ProjectSettings, Variant } from "../types/studio";
import { Button, Dialog, Field, inputClass, Select } from "./ui";
import { ControlSliders } from "./ControlSliders";

export const PLATFORMS: Record<string, { label: string; aspect: string; w: number; h: number; duration: number }> = {
  tiktok: { label: "TikTok", aspect: "9:16", w: 1080, h: 1920, duration: 20 },
  reels: { label: "Instagram Reels", aspect: "9:16", w: 1080, h: 1920, duration: 20 },
  shorts: { label: "YouTube Shorts", aspect: "9:16", w: 1080, h: 1920, duration: 30 },
  youtube: { label: "YouTube (landscape)", aspect: "16:9", w: 1920, h: 1080, duration: 60 },
  square: { label: "Square social", aspect: "1:1", w: 1080, h: 1080, duration: 20 },
  portrait45: { label: "Feed portrait 4:5", aspect: "4:5", w: 1080, h: 1350, duration: 20 },
};

export const VARIANTS: { value: Variant; label: string }[] = [
  { value: "balanced", label: "Balanced" },
  { value: "maximum_impact", label: "Maximum Impact" },
  { value: "smooth_cinematic", label: "Smooth Cinematic" },
  { value: "high_motion", label: "High Motion" },
  { value: "minimal_clean", label: "Minimal Clean" },
  { value: "experimental", label: "Experimental" },
];

export const defaultControls = (): CreativeControls => ({
  beat_sync_strength: 0.5,
  cut_density: 0.5,
  drop_impact: 0.5,
  effect_intensity: 0.5,
  scene_continuity: 0.5,
  motion_energy: 0.5,
  aggressiveness: 0.5,
  transition_frequency: 0.5,
});

export function NewProjectDialog() {
  const { newProjectOpen, setNewProjectOpen, setProject, setRoute, error } = useApp();
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  const niches = useQuery({ queryKey: ["niches"], queryFn: api.listNiches });
  const [name, setName] = useState("Untitled edit");
  const [platform, setPlatform] = useState("tiktok");
  const [fps, setFps] = useState("30");
  const [duration, setDuration] = useState(20);
  const [niche, setNiche] = useState("");
  const [preset, setPreset] = useState("high-energy-edm");
  const [variant, setVariant] = useState<Variant>("balanced");
  const [advanced, setAdvanced] = useState(false);
  const [controls, setControls] = useState<CreativeControls>(defaultControls());
  const [busy, setBusy] = useState(false);

  // Each new project starts from the active profile's defaults (copied, never written back).
  useEffect(() => {
    if (!newProjectOpen) return;
    void api.defaultProjectSettings().then((s) => {
      const pf = Object.entries(PLATFORMS).find(([k, v]) => k === s.platform && v.aspect === s.aspect)?.[0] ?? Object.entries(PLATFORMS).find(([, v]) => v.aspect === s.aspect)?.[0] ?? "tiktok";
      setPlatform(pf);
      setFps(String(s.fps.num));
      setDuration(s.duration_secs);
      setNiche(s.niche_id ?? "");
      setPreset(s.preset_id);
      setVariant(s.variant);
      setControls(s.controls);
      skipPlatformDuration.current = true;
    });
  }, [newProjectOpen]);
  const skipPlatformDuration = useRef(false);
  useEffect(() => {
    if (skipPlatformDuration.current) {
      skipPlatformDuration.current = false;
      return;
    }
    setDuration(PLATFORMS[platform].duration);
  }, [platform]);


  const create = async () => {
    const pf = PLATFORMS[platform];
    const settings: ProjectSettings = {
      platform,
      aspect: pf.aspect,
      width: pf.w,
      height: pf.h,
      fps: { num: parseInt(fps, 10), den: 1 },
      duration_secs: duration,
      niche_id: niche || null,
      preset_id: preset,
      variant,
      seed: Math.floor(Math.random() * 1_000_000) + 1,
      controls,
      cues: [],
      audio_start_secs: null,
      drop_position: null,
      audio_edits: { grid_offset_ms: 0, added_drops_secs: [], removed_drops_secs: [] },
    };
    setBusy(true);
    try {
      const p = await api.createProject(name, settings);
      setProject(p);
      setNewProjectOpen(false);
      setRoute("auto");
    } catch (e) {
      error(e, "Could not create project");
    } finally {
      setBusy(false);
    }
  };

  const categories = Array.from(new Set((niches.data ?? []).map((n) => n.category)));

  return (
    <Dialog
      open={newProjectOpen}
      onClose={() => setNewProjectOpen(false)}
      title="New project"
      width={620}
      footer={
        <>
          <Button variant="ghost" onClick={() => setNewProjectOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || !name.trim()} onClick={create}>
            Create project
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3">
        <div className="col-span-2">
          <Field label="Project name">
            <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </Field>
        </div>
        <Field label="Target platform" hint={`${PLATFORMS[platform].aspect} · ${PLATFORMS[platform].w}×${PLATFORMS[platform].h}`}>
          <Select value={platform} onChange={setPlatform} options={Object.entries(PLATFORMS).map(([k, v]) => ({ value: k, label: v.label }))} />
        </Field>
        <Field label="Frame rate">
          <Select value={fps} onChange={setFps} options={["24", "25", "30", "60"].map((f) => ({ value: f, label: `${f} fps` }))} />
        </Field>
        <Field label="Content niche" hint="Optional; adjusts scene preferences and the default preset.">
          <select className={inputClass} value={niche} onChange={(e) => { setNiche(e.target.value); const n = niches.data?.find((x) => x.id === e.target.value); if (n) setPreset(n.default_preset); }}>
            <option value="">General</option>
            {categories.map((c) => (
              <optgroup key={c} label={c}>
                {(niches.data ?? [])
                  .filter((n) => n.category === c)
                  .map((n) => (
                    <option key={n.id} value={n.id}>
                      {n.name}
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
        </Field>
        <Field label="Editing style (preset)">
          <Select value={preset} onChange={setPreset} options={(presets.data ?? []).map((p) => ({ value: p.id, label: p.name }))} />
        </Field>
        <Field label="Duration (seconds)">
          <input className={inputClass} type="number" min={3} max={600} value={duration} onChange={(e) => setDuration(Math.max(3, Math.min(600, Number(e.target.value) || 20)))} />
        </Field>
        <Field label="Creative variant">
          <Select value={variant} onChange={setVariant} options={VARIANTS} />
        </Field>
      </div>
      <button className="mt-4 text-xs text-muted hover:text-fg" onClick={() => setAdvanced(!advanced)}>
        {advanced ? "▾" : "▸"} Advanced creative controls
      </button>
      {advanced && (
        <div className="mt-3 rounded-lg border border-line p-3">
          <ControlSliders value={controls} onChange={setControls} />
        </div>
      )}
      <p className="mt-4 text-[11px] leading-relaxed text-dim">
        Use music and footage you own or are licensed to use. Commercial promotion usually requires explicit rights for both the track and the video material.
      </p>
    </Dialog>
  );
}
