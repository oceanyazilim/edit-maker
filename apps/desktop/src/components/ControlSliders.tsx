import type { CreativeControls } from "../types/studio";
import { Slider } from "./ui";

// Hints mirror the definitions in docs/PLANNER.md (0.5 = preset as authored).
const DEFS: { key: keyof CreativeControls; label: string; hint: string }[] = [
  { key: "beat_sync_strength", label: "Beat sync strength", hint: "How strongly cut points snap to musical events vs. the target shot length." },
  { key: "cut_density", label: "Cut density", hint: "1.0 halves the target shot length, 0.0 doubles it." },
  { key: "drop_impact", label: "Drop impact", hint: "Scales flash / zoom punch / RGB split / shake on drops." },
  { key: "effect_intensity", label: "Effect intensity", hint: "Scales all automated effects; 0 disables kick-triggered effects." },
  { key: "scene_continuity", label: "Scene continuity", hint: "Preference for continuing the same source and adjacent scenes." },
  { key: "motion_energy", label: "Motion energy", hint: "Shifts footage selection towards higher (or lower) visual energy than the music." },
  { key: "aggressiveness", label: "Editing aggressiveness", hint: "Shorter minimum shots and more kicks qualify for effects." },
  { key: "transition_frequency", label: "Transition frequency", hint: "Probability of soft transitions in sections that allow them." },
];

export function ControlSliders({ value, onChange, cols = 2 }: { value: CreativeControls; onChange: (v: CreativeControls) => void; cols?: 1 | 2 }) {
  return (
    <div className={cols === 2 ? "grid grid-cols-2 gap-x-5 gap-y-3" : "grid grid-cols-1 gap-y-2.5"}>
      {DEFS.map((d) => (
        <Slider key={d.key} label={d.label} hint={d.hint} value={value[d.key]} onChange={(v) => onChange({ ...value, [d.key]: v })} />
      ))}
    </div>
  );
}
