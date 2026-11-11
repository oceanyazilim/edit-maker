import type { Rational, Tick } from "../types/studio";

export const TICKS_PER_SECOND = 705_600_000;

export const ticksToSeconds = (t: Tick): number => t / TICKS_PER_SECOND;
export const secondsToTicks = (s: number): Tick => Math.round(s * TICKS_PER_SECOND);
export const fpsValue = (r: Rational): number => r.num / r.den;

/** Exact start tick of frame `n`. */
export const frameToTicks = (n: number, fps: Rational): Tick => Math.round((n * TICKS_PER_SECOND * fps.den) / fps.num);
/** Frame displayed at tick `t` (floor). */
export const ticksToFrame = (t: Tick, fps: Rational): number => Math.floor((t * fps.num) / (TICKS_PER_SECOND * fps.den) + 1e-9);
export const snapToFrame = (t: Tick, fps: Rational): Tick => frameToTicks(Math.round((t * fps.num) / (TICKS_PER_SECOND * fps.den)), fps);

/** mm:ss.ff (frames) timecode. */
export function timecode(t: Tick, fps: Rational): string {
  const f = fpsValue(fps);
  const frames = ticksToFrame(t, fps);
  const whole = Math.round(f);
  const secs = Math.floor(frames / f);
  const ff = frames - Math.round(secs * f);
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(Math.max(0, Math.min(whole - 1, ff))).padStart(2, "0")}`;
}

export function fmtSeconds(s: number, digits = 2): string {
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  return m > 0 ? `${m}:${r.toFixed(digits).padStart(digits + 3, "0")}` : `${r.toFixed(digits)}s`;
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${u[i]}`;
}

export function fmtDate(unixSecs: number): string {
  return new Date(unixSecs * 1000).toLocaleString();
}
