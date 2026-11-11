import {
  AudioLines,
  Bookmark,
  Eye,
  EyeOff,
  Film,
  Lock,
  LockKeyhole,
  Magnet,
  MousePointer2,
  Plus,
  Redo2,
  Scissors,
  SquareSplitHorizontal,
  Trash2,
  Undo2,
  Unlock,
  Volume2,
  VolumeX,
  ZoomIn,
  ZoomOut,
  Maximize,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { fileUrl } from "../services/api";
import { useApp } from "../stores/app";
import type { Clip, Timeline as TL, Track } from "../types/studio";
import { secondsToTicks, snapToFrame, ticksToSeconds, timecode, TICKS_PER_SECOND } from "../lib/time";
import { songToTimeline, useMusic } from "../lib/music";
import { IconButton, cx } from "../components/ui";
import { RegenTools } from "../components/RegenTools";
import { can, cmd } from "../shell/commands";

const SECTION_COLOR: Record<string, string> = {
  intro: "#2b3240",
  groove: "#2c3a4c",
  build_up: "#5a4522",
  drop: "#6a2c31",
  breakdown: "#24443e",
  outro: "#30303a",
};
const HEADER_W = 148;
const RULER_H = 24;
const GRID_H = 22;
const ROW_H: Record<string, number> = { video: 58, audio: 46, text: 30 };

type Drag =
  | { kind: "move"; clip: Clip; track: Track; startX: number; orig: number; delta: number }
  | { kind: "trim-start" | "trim-end"; clip: Clip; track: Track; startX: number; orig: number; delta: number }
  | { kind: "scrub" }
  | { kind: "range"; anchor: number };

export function Timeline() {
  const project = useApp((s) => s.project);
  const playhead = useApp((s) => s.playhead);
  const setPlayhead = useApp((s) => s.setPlayhead);
  const selectedClipId = useApp((s) => s.selectedClipId);
  const selectClip = useApp((s) => s.selectClip);
  const editTimeline = useApp((s) => s.editTimeline);
  const range = useApp((s) => s.range);
  const setRange = useApp((s) => s.setRange);
  const pps = useApp((s) => s.tlZoom);
  const setPps = useApp((s) => s.setTlZoom);
  const tool = useApp((s) => s.tlTool);
  const setTool = useApp((s) => s.setTlTool);
  const snapOn = useApp((s) => s.snapOn);
  const setSnapOn = useApp((s) => s.setSnapOn);
  const canUndo = useApp((s) => s.canUndo);
  const canRedo = useApp((s) => s.canRedo);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [razorX, setRazorX] = useState<number | null>(null);
  const [trackMenu, setTrackMenu] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const tl = project?.timeline as TL | undefined;
  const { clip: musicClip, analysis, wave } = useMusic(project);

  const fps = tl?.settings.fps ?? { num: 30, den: 1 };
  const duration = tl ? tl.duration ?? Math.max(0, ...tl.tracks.flatMap((t) => t.clips.map((c) => c.start + c.duration))) : 0;
  const px = (t: number) => (t / TICKS_PER_SECOND) * pps;
  const fromPx = (x: number) => secondsToTicks(x / pps);
  const width = Math.max(px(duration) + 240, 600);

  // Musical events on the timeline (only the part of the song that is used).
  const grid = useMemo(() => {
    const empty = { beats: [] as { t: number; down: boolean; bar: number }[], kicks: [] as { t: number; i: number }[], drops: [] as { t: number; main: boolean }[], sections: [] as { a: number; b: number; kind: string }[] };
    if (!analysis || !musicClip) return empty;
    const sr = analysis.sample_rate;
    const map = (s: number) => songToTimeline(s / sr, musicClip);
    for (const b of analysis.beats) {
      const t = map(b.sample);
      if (t != null) empty.beats.push({ t, down: b.is_downbeat, bar: b.bar });
    }
    for (const k of analysis.kicks) {
      const t = map(k.sample);
      if (t != null) empty.kicks.push({ t, i: k.intensity });
    }
    for (const d of analysis.drops) {
      if (d.kind === "false_drop_candidate") continue;
      const t = map(d.sample);
      if (t != null) empty.drops.push({ t, main: d.kind === "main_drop" });
    }
    const lo = musicClip.start;
    const hi = musicClip.start + musicClip.duration;
    for (const s of analysis.sections) {
      const a = songToTimeline(s.start_sample / sr, musicClip, true)!;
      const b = songToTimeline(s.end_sample / sr, musicClip, true)!;
      if (b > lo && a < hi) empty.sections.push({ a: Math.max(lo, a), b: Math.min(hi, b), kind: s.kind });
    }
    return empty;
  }, [analysis, musicClip]);

  // Snap targets: clip edges, markers, playhead, beats.
  const snapPoints = useMemo(() => {
    if (!tl) return [] as number[];
    const pts = [0, playhead, ...tl.markers.map((m) => m.t), ...grid.beats.map((b) => b.t), ...grid.drops.map((d) => d.t)];
    tl.tracks.forEach((t) => t.clips.forEach((c) => pts.push(c.start, c.start + c.duration)));
    return pts;
  }, [tl, playhead, grid]);
  const snap = (t: number, exclude?: number[]) => {
    if (!snapOn) return snapToFrame(t, fps);
    const tol = fromPx(7);
    let best = t;
    let bd = tol;
    for (const p of snapPoints) {
      if (exclude?.includes(p)) continue;
      const d = Math.abs(p - t);
      if (d < bd) {
        bd = d;
        best = p;
      }
    }
    return snapToFrame(best, fps);
  };
  const tAtClient = (clientX: number) => {
    const r = scrollRef.current!.getBoundingClientRect();
    return Math.max(0, fromPx(clientX - r.left + scrollRef.current!.scrollLeft - HEADER_W));
  };

  // Global drag handling.
  useEffect(() => {
    if (!drag) return;
    const move = (e: MouseEvent) => {
      if (drag.kind === "scrub") setPlayhead(snapToFrame(tAtClient(e.clientX), fps));
      else if (drag.kind === "range") {
        const t = snap(tAtClient(e.clientX));
        setRange({ start: Math.min(drag.anchor, t), end: Math.max(drag.anchor, t) });
      } else setDrag({ ...drag, delta: fromPx(e.clientX - drag.startX) });
    };
    const up = async () => {
      const d = drag;
      setDrag(null);
      if (d.kind === "range") {
        const r = useApp.getState().range;
        if (r && r.end - r.start < fromPx(4)) setRange(null);
        return;
      }
      if (d.kind === "scrub" || Math.abs(d.delta) < fromPx(2)) return;
      if (d.kind === "move") {
        const ns = Math.max(0, snap(d.orig + d.delta, [d.clip.start, d.clip.start + d.clip.duration]));
        await editTimeline({ op: "move", clip_id: d.clip.id, track_id: d.track.id, new_start: ns });
      } else if (d.kind === "trim-start") {
        await editTimeline({ op: "trim_start", clip_id: d.clip.id, new_start: snap(d.orig + d.delta, [d.clip.start]) });
      } else {
        await editTimeline({ op: "trim_end", clip_id: d.clip.id, new_end: snap(d.orig + d.delta, [d.clip.start + d.clip.duration]) });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up, { once: true });
    // Remove both listeners: this effect re-runs on every drag update, and a stale
    // mouseup handler would commit the edit once per render.
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
  }); // re-bind with fresh closures

  // Timeline shortcuts (global ones live in the menu bar).
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || !tl || e.ctrlKey || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "s") void cmd.split();
      else if (e.key === "Delete" || e.key === "Backspace") void cmd.deleteSelected(e.shiftKey);
      else if (key === "l") cmd.toggleLock();
      else if (e.key === "Escape") setRange(null);
      else if (e.key === "=" || e.key === "+") cmd.zoomIn();
      else if (e.key === "-") cmd.zoomOut();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  // Fit to window.
  useEffect(() => {
    const fit = () => {
      const el = scrollRef.current;
      if (!el || duration <= 0) return;
      setPps((el.clientWidth - HEADER_W - 40) / ticksToSeconds(duration));
      el.scrollLeft = 0;
    };
    window.addEventListener("studio:timeline-fit", fit);
    return () => window.removeEventListener("studio:timeline-fit", fit);
  }, [duration, setPps]);

  // Keep the playhead visible.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || drag) return;
    const x = px(playhead) + HEADER_W;
    if (x < el.scrollLeft + HEADER_W || x > el.scrollLeft + el.clientWidth - 40) el.scrollLeft = Math.max(0, x - HEADER_W - 80);
  }, [playhead]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!tl) return null;

  const rulerStep = pps >= 240 ? 0.25 : pps >= 120 ? 0.5 : pps >= 50 ? 1 : pps >= 22 ? 2 : pps >= 10 ? 5 : 10;
  const ticks: number[] = [];
  for (let s = 0; s <= ticksToSeconds(duration) + 4; s += rulerStep) ticks.push(Math.round(s * 1000) / 1000);
  const beatPx = grid.beats.length > 1 ? px(grid.beats[1].t - grid.beats[0].t) : 99;

  const clipGeom = (c: Clip) => {
    let start = c.start;
    let dur = c.duration;
    if (drag && drag.kind !== "scrub" && drag.kind !== "range" && drag.clip.id === c.id) {
      if (drag.kind === "move") start = Math.max(0, drag.orig + drag.delta);
      if (drag.kind === "trim-start") {
        start = Math.min(c.start + c.duration - secondsToTicks(0.05), drag.orig + drag.delta);
        dur = c.start + c.duration - start;
      }
      if (drag.kind === "trim-end") dur = Math.max(secondsToTicks(0.05), drag.orig + drag.delta - c.start);
    }
    return { left: px(start), width: Math.max(2, px(dur)) };
  };

  const sel = tl.tracks.flatMap((t) => t.clips.map((c) => ({ c, t }))).find((x) => x.c.id === selectedClipId);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Toolbar */}
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-b border-line px-2">
        <IconButton title="Undo (Ctrl+Z)" disabled={!canUndo} onClick={cmd.undo}>
          <Undo2 size={14} />
        </IconButton>
        <IconButton title="Redo (Ctrl+Y)" disabled={!canRedo} onClick={cmd.redo}>
          <Redo2 size={14} />
        </IconButton>
        <Divider />
        <IconButton title="Select tool (V)" active={tool === "select"} onClick={() => setTool("select")}>
          <MousePointer2 size={14} />
        </IconButton>
        <IconButton title="Razor tool (C) — click a clip to cut it" active={tool === "razor"} onClick={() => setTool("razor")}>
          <Scissors size={14} />
        </IconButton>
        <IconButton title={snapOn ? "Snapping on: beats, markers, clip edges" : "Snapping off"} active={snapOn} onClick={() => setSnapOn(!snapOn)}>
          <Magnet size={14} />
        </IconButton>
        <Divider />
        <IconButton title="Split at playhead (S)" disabled={!can.split()} onClick={() => cmd.split()}>
          <SquareSplitHorizontal size={14} />
        </IconButton>
        <IconButton title="Delete selected clip (Del) · Shift+Del ripple" disabled={!sel || sel.c.locked || sel.t.locked} onClick={() => cmd.deleteSelected(false)}>
          <Trash2 size={14} />
        </IconButton>
        <IconButton title="Add marker at playhead (M)" onClick={cmd.addMarker}>
          <Bookmark size={14} />
        </IconButton>
        <div className="relative">
          <IconButton title="Add track" active={trackMenu} onClick={() => setTrackMenu(!trackMenu)}>
            <Plus size={14} />
          </IconButton>
          {trackMenu && (
            <div className="absolute left-0 top-full z-40 mt-1 w-36 rounded-md border border-line bg-surface p-1 shadow-xl" onMouseLeave={() => setTrackMenu(false)}>
              <button className="flex h-7 w-full items-center gap-2 rounded px-2 text-xs hover:bg-raised" onClick={() => { setTrackMenu(false); cmd.addTrack("video"); }}>
                <Film size={12} /> Video track
              </button>
              <button className="flex h-7 w-full items-center gap-2 rounded px-2 text-xs hover:bg-raised" onClick={() => { setTrackMenu(false); cmd.addTrack("audio"); }}>
                <AudioLines size={12} /> Audio track
              </button>
            </div>
          )}
        </div>
        <RegenTools />
        <div className="flex-1" />
        <span className="tabular mr-3 rounded bg-base px-2 py-0.5 font-mono text-[11px] text-fg">{timecode(playhead, fps)}</span>
        <IconButton title="Zoom out (-)" onClick={cmd.zoomOut}>
          <ZoomOut size={14} />
        </IconButton>
        <input
          type="range"
          aria-label="Timeline zoom"
          min={Math.log(4)}
          max={Math.log(800)}
          step={0.01}
          value={Math.log(pps)}
          onChange={(e) => setPps(Math.exp(Number(e.target.value)))}
          className="mx-1 w-24"
        />
        <IconButton title="Zoom in (+)" onClick={cmd.zoomIn}>
          <ZoomIn size={14} />
        </IconButton>
        <IconButton title="Fit timeline to window (Shift+Z)" onClick={cmd.zoomFit}>
          <Maximize size={13} />
        </IconButton>
      </div>

      <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-auto">
        <div style={{ width: width + HEADER_W }} className="relative">
          {/* Ruler */}
          <div className="sticky top-0 z-20 flex border-b border-line bg-surface" style={{ height: RULER_H }}>
            <div className="sticky left-0 z-30 flex shrink-0 items-center border-r border-line bg-surface px-2 text-[10px] text-dim" style={{ width: HEADER_W }}>
              {ticksToSeconds(duration).toFixed(2)}s · {tl.tracks.length} tracks
            </div>
            <div
              className="relative flex-1 cursor-text"
              title="Click/drag: move playhead · Shift+drag: select a range"
              onMouseDown={(e) => {
                const t = snapToFrame(tAtClient(e.clientX), fps);
                if (e.shiftKey) {
                  setRange({ start: t, end: t });
                  setDrag({ kind: "range", anchor: t });
                } else {
                  setDrag({ kind: "scrub" });
                  setPlayhead(t);
                }
              }}
            >
              {ticks.map((s) => (
                <div key={s} className="absolute bottom-0 h-2 border-l border-line-strong" style={{ left: s * pps }}>
                  <span className="tabular absolute -top-3.5 left-1 whitespace-nowrap text-[9px] text-dim">{rulerStep < 1 ? s.toFixed(2) : s.toFixed(0)}s</span>
                </div>
              ))}
              {tl.markers.map((m) => (
                <button
                  key={m.id}
                  className="absolute top-0 z-10 h-full"
                  style={{ left: px(m.t) - 5 }}
                  title={`${m.label} — click: go to · Alt+click: remove`}
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    if (e.altKey && m.kind === "user") void cmd.removeMarker(m.id);
                    else setPlayhead(m.t);
                  }}
                >
                  <svg width="10" height="12" viewBox="0 0 10 12" className="block">
                    <path d="M0 0h10v8l-5 4-5-4z" fill={m.kind === "drop" ? "#f26d6d" : m.kind === "user" ? "#4387ef" : "#7aa7f5"} />
                  </svg>
                </button>
              ))}
            </div>
          </div>

          {/* Beat grid */}
          <div className="sticky z-[19] flex border-b border-line bg-surface" style={{ top: RULER_H, height: GRID_H }}>
            <div className="sticky left-0 z-30 flex shrink-0 items-center gap-1.5 border-r border-line bg-surface px-2 text-[10px] text-muted" style={{ width: HEADER_W }}>
              Beat grid
              {analysis && musicClip ? <span className="text-dim">{analysis.tempo.bpm.toFixed(1)} BPM</span> : <span className="text-dim">{analysis ? "no music clip" : "not analyzed"}</span>}
            </div>
            <div className="relative flex-1 overflow-hidden">
              {grid.sections.map((s, i) => (
                <div key={i} className="absolute inset-y-0" style={{ left: px(s.a), width: px(s.b - s.a), background: SECTION_COLOR[s.kind] ?? "#2a2a33", opacity: 0.55 }} title={s.kind.replace("_", " ")}>
                  {px(s.b - s.a) > 50 && <span className="absolute left-1 top-0.5 text-[9px] uppercase tracking-wide text-white/55">{s.kind.replace("_", " ")}</span>}
                </div>
              ))}
              {grid.beats.map((b, i) =>
                b.down || beatPx > 6 ? (
                  <div key={i} className={cx("absolute bottom-0 w-px", b.down ? "h-full bg-white/35" : "h-1.5 bg-white/25")} style={{ left: px(b.t) }}>
                    {b.down && beatPx * 4 > 26 && <span className="tabular absolute bottom-0 left-0.5 text-[8px] text-white/45">{b.bar + 1}</span>}
                  </div>
                ) : null,
              )}
              {grid.kicks.map((k, i) => (
                <div key={i} className="absolute bottom-1.5 h-1.5 w-1.5 -translate-x-1/2 rounded-full bg-kick" style={{ left: px(k.t), opacity: 0.4 + 0.6 * Math.min(1, k.i) }} title="kick" />
              ))}
              {grid.drops.map((d, i) => (
                <div key={i} className="absolute inset-y-0 w-0.5 bg-drop" style={{ left: px(d.t) }} title={d.main ? "main drop" : "drop"}>
                  <span className="absolute left-1 top-0.5 text-[9px] font-semibold text-drop">{d.main ? "DROP" : "drop"}</span>
                </div>
              ))}
            </div>
          </div>

          {/* Tracks */}
          {tl.tracks.map((track) => (
            <div key={track.id} className="flex border-b border-line" style={{ height: ROW_H[track.kind] }}>
              <div className="sticky left-0 z-10 flex shrink-0 items-center gap-1.5 border-r border-line bg-surface pl-2 pr-1" style={{ width: HEADER_W }}>
                <span className={cx("flex h-5 w-6 shrink-0 items-center justify-center rounded text-[9px] font-semibold", track.kind === "audio" ? "bg-ok/15 text-ok" : "bg-accent/15 text-accent")}>{track.id}</span>
                <span className="min-w-0 flex-1 truncate text-[11px] text-muted" title={track.name}>
                  {track.name}
                </span>
                <IconButton title={track.locked ? "Unlock track" : "Lock track"} active={track.locked} onClick={() => editTimeline({ op: "set_track_flags", track_id: track.id, locked: !track.locked })}>
                  {track.locked ? <Lock size={11} /> : <Unlock size={11} />}
                </IconButton>
                {track.kind === "audio" ? (
                  <IconButton title={track.muted ? "Unmute" : "Mute"} active={track.muted} onClick={() => editTimeline({ op: "set_track_flags", track_id: track.id, muted: !track.muted })}>
                    {track.muted ? <VolumeX size={11} /> : <Volume2 size={11} />}
                  </IconButton>
                ) : (
                  <IconButton title={track.hidden ? "Show track" : "Hide track"} active={track.hidden} onClick={() => editTimeline({ op: "set_track_flags", track_id: track.id, hidden: !track.hidden })}>
                    {track.hidden ? <EyeOff size={11} /> : <Eye size={11} />}
                  </IconButton>
                )}
              </div>
              <div
                className={cx("relative flex-1", (track.hidden || track.muted) && "opacity-40")}
                onMouseDown={(e) => {
                  if (e.target === e.currentTarget) {
                    selectClip(null);
                    setPlayhead(snapToFrame(tAtClient(e.clientX), fps));
                  }
                }}
              >
                {track.clips.map((c) => {
                  const g = clipGeom(c);
                  const isSel = c.id === selectedClipId;
                  const asset = project!.assets.find((a) => a.id === c.asset_id);
                  const kickFx = c.effects.filter((e) => e.trigger === "kick" || e.trigger === "drop");
                  const razor = tool === "razor" && !track.locked && !c.locked;
                  return (
                    <div
                      key={c.id}
                      className={cx(
                        "group absolute top-1 bottom-1 overflow-hidden rounded-[4px] border text-[10px]",
                        isSel ? "border-accent ring-1 ring-accent" : c.locked ? "border-accent/45" : "border-white/10",
                        razor ? "cursor-crosshair" : track.locked ? "cursor-not-allowed" : "cursor-grab",
                        asset?.missing && "outline outline-1 outline-err",
                      )}
                      style={{ left: g.left, width: g.width, background: track.kind === "audio" ? "#1d3a33" : "#1d2738" }}
                      title={`${asset?.name ?? c.asset_id}${c.origin.reason ? `\n${c.origin.reason}` : ""}${c.locked ? "\n🔒 locked" : ""}`}
                      onMouseMove={(e) => razor && setRazorX(e.clientX)}
                      onMouseLeave={() => setRazorX(null)}
                      onMouseDown={(e) => {
                        e.stopPropagation();
                        if (razor) {
                          const at = snap(tAtClient(e.clientX));
                          if (at > c.start && at < c.start + c.duration) void cmd.splitClipAt(c.id, at);
                          return;
                        }
                        selectClip(c.id);
                        if (!track.locked && !c.locked) setDrag({ kind: "move", clip: c, track, startX: e.clientX, orig: c.start, delta: 0 });
                      }}
                    >
                      {track.kind === "video" && asset?.thumbnail && (
                        <div className="absolute inset-0 opacity-70" style={{ backgroundImage: `url("${fileUrl(asset.thumbnail)}")`, backgroundSize: "auto 100%", backgroundRepeat: "repeat-x" }} />
                      )}
                      {track.kind === "video" && <div className="absolute inset-0 bg-gradient-to-b from-black/55 via-transparent to-black/40" />}
                      {track.kind === "audio" && wave && musicClip?.id === c.id && <Waveform peaks={wave.peaks} spb={wave.samples_per_bucket} sr={wave.sample_rate} sourceIn={c.source_in} duration={c.duration} width={g.width} />}
                      <div className="relative flex items-center gap-1 truncate px-1.5 pt-0.5 font-medium text-white/90">
                        {c.locked && <LockKeyhole size={10} className="shrink-0 text-accent" aria-label="locked" />}
                        {c.speed !== 1 && <span className="rounded bg-black/50 px-0.5 text-[9px] text-kick">{c.speed.toFixed(2)}×</span>}
                        <span className="truncate">{asset?.name ?? "missing media"}</span>
                      </div>
                      {track.kind === "video" && c.transition_in && <div className="absolute bottom-0 left-0 top-0 w-2 bg-gradient-to-r from-white/40 to-transparent" title={c.transition_in.kind.replace(/_/g, " ")} />}
                      {track.kind === "video" && c.origin.section && <div className="absolute inset-x-0 bottom-0 h-[3px]" style={{ background: SECTION_COLOR[c.origin.section] ?? "transparent" }} />}
                      {kickFx.map((e) => (
                        <div key={e.id} className={cx("absolute bottom-[3px] h-2 w-0.5", e.trigger === "drop" ? "bg-drop" : "bg-kick")} style={{ left: px(e.offset) }} />
                      ))}
                      {!track.locked && !c.locked && tool === "select" && (
                        <>
                          <div
                            className="absolute inset-y-0 left-0 w-1.5 cursor-ew-resize hover:bg-accent/60"
                            onMouseDown={(e) => {
                              e.stopPropagation();
                              selectClip(c.id);
                              setDrag({ kind: "trim-start", clip: c, track, startX: e.clientX, orig: c.start, delta: 0 });
                            }}
                          />
                          <div
                            className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize hover:bg-accent/60"
                            onMouseDown={(e) => {
                              e.stopPropagation();
                              selectClip(c.id);
                              setDrag({ kind: "trim-end", clip: c, track, startX: e.clientX, orig: c.start + c.duration, delta: 0 });
                            }}
                          />
                        </>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
          {/* Empty space below tracks: click to deselect */}
          <div className="h-10" onMouseDown={() => selectClip(null)} />

          {/* Range selection */}
          {range && <div className="pointer-events-none absolute bottom-0 top-0 z-10 border-x border-accent/70 bg-accent/10" style={{ left: HEADER_W + px(range.start), width: Math.max(1, px(range.end - range.start)) }} />}
          {/* Razor guide */}
          {razorX != null && scrollRef.current && (
            <div className="pointer-events-none absolute bottom-0 top-0 z-20 w-px bg-white/70" style={{ left: razorX - scrollRef.current.getBoundingClientRect().left + scrollRef.current.scrollLeft }} />
          )}
          {/* Playhead */}
          <div className="pointer-events-none absolute bottom-0 top-0 z-[25] w-px bg-white" style={{ left: HEADER_W + px(playhead) }}>
            <div className="absolute -left-[5px] top-0 h-0 w-0 border-x-[5px] border-t-[7px] border-x-transparent border-t-white" />
          </div>
        </div>
      </div>
    </div>
  );
}

function Divider() {
  return <div className="mx-1 h-4 w-px bg-line" />;
}

function Waveform({ peaks, spb, sr, sourceIn, duration, width }: { peaks: number[]; spb: number; sr: number; sourceIn: number; duration: number; width: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const w = Math.min(Math.max(1, Math.floor(width)), 16000);
    const h = 34;
    cv.width = w;
    cv.height = h;
    const ctx = cv.getContext("2d")!;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "rgba(62,207,142,0.6)";
    const b0 = Math.floor(((sourceIn / TICKS_PER_SECOND) * sr) / spb);
    const nb = Math.max(1, Math.floor(((duration / TICKS_PER_SECOND) * sr) / spb));
    for (let x = 0; x < w; x++) {
      const i0 = b0 + Math.floor((x / w) * nb);
      const i1 = Math.max(i0 + 1, b0 + Math.floor(((x + 1) / w) * nb));
      let lo = 0;
      let hi = 0;
      for (let i = i0; i < i1 && 2 * i + 1 < peaks.length; i++) {
        lo = Math.min(lo, peaks[2 * i]);
        hi = Math.max(hi, peaks[2 * i + 1]);
      }
      const y0 = h / 2 - (hi / 127) * (h / 2);
      const y1 = h / 2 - (lo / 127) * (h / 2);
      ctx.fillRect(x, y0, 1, Math.max(1, y1 - y0));
    }
  }, [peaks, spb, sr, sourceIn, duration, width]);
  return <canvas ref={ref} className="absolute inset-x-0 bottom-0 h-[34px] w-full" />;
}
