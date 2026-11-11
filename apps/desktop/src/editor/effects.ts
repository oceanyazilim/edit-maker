import type { Clip, EffectInstance, EffectKind, TransitionKind } from "../types/studio";
import { secondsToTicks } from "../lib/time";

/** Effects the Rust compositor renders (same code for preview and export). */
export const EFFECTS: { type: EffectKind["type"]; label: string; description: string; wholeClip?: boolean; make: () => EffectKind }[] = [
  { type: "zoom_punch", label: "Zoom punch", description: "Quick scale hit that decays — classic kick accent.", make: () => ({ type: "zoom_punch", amount: 0.08 }) },
  { type: "shake", label: "Camera shake", description: "Deterministic handheld-style shake.", make: () => ({ type: "shake", amplitude: 0.01, frequency_hz: 12, seed: Math.floor(Math.random() * 1e6) }) },
  { type: "flash", label: "Flash", description: "White flash that fades out.", make: () => ({ type: "flash", strength: 0.7 }) },
  { type: "rgb_split", label: "RGB split", description: "Offsets the colour channels.", make: () => ({ type: "rgb_split", offset: 0.01 }) },
  { type: "glitch", label: "Glitch", description: "Block displacement and channel tearing.", make: () => ({ type: "glitch", strength: 0.6, seed: Math.floor(Math.random() * 1e6) }) },
  { type: "chromatic_aberration", label: "Chromatic aberration", description: "Radial colour fringing towards the edges.", make: () => ({ type: "chromatic_aberration", amount: 0.02 }) },
  { type: "motion_blur", label: "Motion blur", description: "Directional blur along an angle.", make: () => ({ type: "motion_blur", amount: 0.04, angle_deg: 0 }) },
  { type: "zoom_blur", label: "Zoom blur", description: "Radial blur from the centre.", make: () => ({ type: "zoom_blur", amount: 0.1 }) },
  { type: "color", label: "Color", description: "Brightness, contrast and saturation for the whole clip.", wholeClip: true, make: () => ({ type: "color", brightness: 0, contrast: 1.1, saturation: 1.1 }) },
  { type: "vignette", label: "Vignette", description: "Darkens the edges for the whole clip.", wholeClip: true, make: () => ({ type: "vignette", strength: 0.3 }) },
];

export const PARAMS: Record<string, { key: string; label: string; min: number; max: number; step?: number }[]> = {
  zoom_punch: [{ key: "amount", label: "Amount", min: 0, max: 0.5 }],
  shake: [
    { key: "amplitude", label: "Amplitude", min: 0, max: 0.05, step: 0.001 },
    { key: "frequency_hz", label: "Frequency (Hz)", min: 1, max: 30, step: 0.5 },
  ],
  flash: [{ key: "strength", label: "Strength", min: 0, max: 1 }],
  rgb_split: [{ key: "offset", label: "Offset", min: 0, max: 0.05, step: 0.001 }],
  color: [
    { key: "brightness", label: "Brightness", min: -0.5, max: 0.5 },
    { key: "contrast", label: "Contrast", min: 0.5, max: 2 },
    { key: "saturation", label: "Saturation", min: 0, max: 2 },
  ],
  vignette: [{ key: "strength", label: "Strength", min: 0, max: 1 }],
  fade: [],
  glitch: [{ key: "strength", label: "Strength", min: 0, max: 1 }],
  chromatic_aberration: [{ key: "amount", label: "Amount", min: 0, max: 0.1, step: 0.001 }],
  motion_blur: [
    { key: "amount", label: "Length", min: 0, max: 0.2, step: 0.002 },
    { key: "angle_deg", label: "Angle", min: -180, max: 180, step: 1 },
  ],
  zoom_blur: [{ key: "amount", label: "Amount", min: 0, max: 0.3, step: 0.005 }],
};

/** A clip with a new effect at the playhead (or spanning the clip for whole-clip effects). */
export function withEffect(clip: Clip, def: (typeof EFFECTS)[number], playhead: number): Clip {
  const off = def.wholeClip ? 0 : Math.max(0, Math.min(clip.duration - secondsToTicks(0.1), playhead - clip.start));
  const dur = def.wholeClip ? clip.duration : Math.min(secondsToTicks(0.4), clip.duration - off);
  const fx: EffectInstance = { id: `fx_user_${Date.now()}_${Math.floor(Math.random() * 1e4)}`, kind: def.make(), offset: off, duration: dur, version: 1, trigger: "user" };
  return { ...clip, effects: [...clip.effects, fx] };
}

export const TRANSITIONS: { value: TransitionKind | "cut"; label: string; description: string }[] = [
  { value: "cut", label: "Hard cut", description: "No transition." },
  { value: "cross_dissolve", label: "Cross dissolve", description: "Blend from the previous clip." },
  { value: "flash_cut", label: "Flash cut", description: "Cut through a white flash." },
  { value: "zoom_blend", label: "Zoom blend", description: "Zoom into the cut while blending." },
  { value: "dip_to_black", label: "Dip to black", description: "Fade through black." },
];
