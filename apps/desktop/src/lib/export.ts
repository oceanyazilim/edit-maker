// Pure helpers for the export dialog (resolution math, Windows file-name rules).
// The Rust side re-validates everything; these only give immediate feedback.

export type ResPreset = "720" | "1080" | "1440" | "2160" | "custom";
export const RES_PRESETS: { value: ResPreset; label: string; short?: number }[] = [
  { value: "720", label: "720p", short: 720 },
  { value: "1080", label: "1080p", short: 1080 },
  { value: "1440", label: "1440p", short: 1440 },
  { value: "2160", label: "4K", short: 2160 },
  { value: "custom", label: "Custom" },
];

export const FPS_OPTIONS: { label: string; num: number; den: number }[] = [
  { label: "24", num: 24, den: 1 },
  { label: "25", num: 25, den: 1 },
  { label: "30", num: 30, den: 1 },
  { label: "50", num: 50, den: 1 },
  { label: "59.94", num: 60000, den: 1001 },
  { label: "60", num: 60, den: 1 },
];

const even = (x: number) => Math.max(2, Math.round(x / 2) * 2);

/** Parse "9:16" → [9, 16]; null when malformed. */
export function parseAspect(a: string): [number, number] | null {
  const m = /^\s*(\d+(?:\.\d+)?)\s*:\s*(\d+(?:\.\d+)?)\s*$/.exec(a);
  if (!m) return null;
  const w = Number(m[1]);
  const h = Number(m[2]);
  return w > 0 && h > 0 ? [w, h] : null;
}

/**
 * Canvas size for a "short side" preset at an aspect ratio: 1080p at 9:16 is 1080×1920,
 * at 16:9 it is 1920×1080. Both sides are even (H.264 4:2:0 requirement).
 */
export function sizeFor(short: number, aspectW: number, aspectH: number): { width: number; height: number } {
  if (aspectW >= aspectH) return { width: even((short * aspectW) / aspectH), height: even(short) };
  return { width: even(short), height: even((short * aspectH) / aspectW) };
}

/** The preset matching a canvas exactly, else "custom". */
export function presetOf(width: number, height: number): ResPreset {
  const short = Math.min(width, height);
  const p = RES_PRESETS.find((r) => r.short === short);
  return p ? p.value : "custom";
}

export function validDimension(v: number): boolean {
  return Number.isInteger(v) && v >= 16 && v <= 8192 && v % 2 === 0;
}

const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/** Error text for an invalid Windows file name (without folder), or null when valid. */
export function fileNameError(name: string): string | null {
  if (!name.trim()) return "Enter a file name.";
  if (name.length > 200) return "The file name is too long.";
  // eslint-disable-next-line no-control-regex
  if (/[<>:"/\\|?*\u0000-\u001f]/.test(name)) return 'File names cannot contain < > : " / \\ | ? * or control characters.';
  if (/[ .]$/.test(name)) return "File names cannot end with a space or a dot.";
  const stem = name.replace(/\.[^.]*$/, "");
  if (RESERVED.test(stem)) return `"${stem}" is a reserved name on Windows.`;
  return null;
}

/** Ensure `name` ends with `.ext` (case-insensitive), replacing a different media extension. */
export function withExt(name: string, ext: string): string {
  const n = name.trim();
  if (n.toLowerCase().endsWith(`.${ext}`)) return n;
  const stripped = n.replace(/\.(mp4|m4a|wav|png|jpe?g)$/i, "");
  return `${stripped}.${ext}`;
}

export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  const sep = dir.includes("\\") || !dir.includes("/") ? "\\" : "/";
  return dir.endsWith("\\") || dir.endsWith("/") ? dir + name : dir + sep + name;
}

/** "9:16" → "9x16" (file-name safe suffix). */
export const aspectSuffix = (a: string) => a.replace(":", "x");
