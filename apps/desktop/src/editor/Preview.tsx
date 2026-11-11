import { Film, Fullscreen, Grid2x2, Loader2, Pause, Play, SkipBack, SkipForward, Square, Volume2, VolumeX } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, fileUrl } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import { frameToTicks, secondsToTicks, ticksToFrame, ticksToSeconds, timecode } from "../lib/time";
import { Button, EmptyState, IconButton, cx } from "../components/ui";

type Quality = "quarter" | "half" | "full";
const SCALE: Record<Quality, number> = { quarter: 0.25, half: 0.5, full: 1 };

/**
 * Preview = (a) frame-accurate stills composited by the Rust render engine (same code as export)
 * and (b) playback of a low-resolution preview render, whose media clock drives the playhead.
 */
export function Preview({ compact = false }: { compact?: boolean }) {
  const project = useApp((s) => s.project);
  const playhead = useApp((s) => s.playhead);
  const setPlayhead = useApp((s) => s.setPlayhead);
  const error = useApp((s) => s.error);
  const [quality, setQuality] = useState<Quality>("half");
  const [frameUrl, setFrameUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [safe, setSafe] = useState(false);
  const [zoom, setZoom] = useState<"fit" | "50" | "100">("fit");
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [rate, setRate] = useState(1);
  const videoRef = useRef<HTMLVideoElement>(null);
  const seq = useRef(0);
  const boxRef = useRef<HTMLDivElement>(null);
  const tl = project?.timeline;
  const fps = tl?.settings.fps ?? { num: 30, den: 1 };
  const duration = tl ? tl.duration ?? Math.max(0, ...tl.tracks.flatMap((t) => t.clips.map((c) => c.start + c.duration))) : 0;
  const hasClips = !!tl?.tracks.some((t) => t.kind === "video" && t.clips.length);

  // Timeline changed → the preview render is stale.
  const tlKey = tl ? JSON.stringify(tl.tracks.map((t) => t.clips.map((c) => [c.id, c.start, c.duration, c.source_in, c.effects.length, c.transition_in?.kind]))) : "";
  useEffect(() => {
    setVideoUrl(null);
    setPlaying(false);
  }, [tlKey]);

  // Still frame at the playhead (debounced; obsolete requests are discarded).
  useEffect(() => {
    if (!hasClips || playing) return;
    const my = ++seq.current;
    const t = setTimeout(async () => {
      setLoading(true);
      try {
        const url = await api.previewFrame(Math.min(playhead, Math.max(0, duration - 1)), SCALE[quality]);
        if (my === seq.current) {
          setFrameUrl((old) => {
            if (old) URL.revokeObjectURL(old);
            return url;
          });
        } else URL.revokeObjectURL(url);
      } catch (e) {
        if (my === seq.current) error(e, "Preview failed");
      } finally {
        if (my === seq.current) setLoading(false);
      }
    }, 90);
    return () => clearTimeout(t);
  }, [playhead, quality, hasClips, playing, tlKey, duration, error]);

  // Media clock → playhead.
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    const tick = () => {
      const v = videoRef.current;
      if (v) setPlayhead(secondsToTicks(v.currentTime));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, setPlayhead]);

  const renderPreview = async () => {
    setRendering(true);
    try {
      const id = await api.renderPreview();
      const job = await waitForJob(id);
      setVideoUrl(fileUrl(job.result as string));
    } catch (e) {
      error(e, "Preview render failed");
    } finally {
      setRendering(false);
    }
  };

  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;
    v.volume = volume;
    v.muted = muted;
    v.playbackRate = rate;
  }, [volume, muted, rate, videoUrl]);

  useEffect(() => {
    const h = () => {
      if (!rendering) void renderPreview();
    };
    window.addEventListener("studio:render-preview", h);
    return () => window.removeEventListener("studio:render-preview", h);
  });

  const fullscreen = () => {
    const el = boxRef.current;
    if (!el) return;
    if (document.fullscreenElement) void document.exitFullscreen();
    else void el.requestFullscreen().catch((e) => error(e, "Fullscreen is not available"));
  };

  const togglePlay = useCallback(() => {
    const v = videoRef.current;
    if (!videoUrl || !v) return;
    if (v.paused) {
      v.currentTime = ticksToSeconds(playhead);
      void v.play();
      setPlaying(true);
    } else {
      v.pause();
      setPlaying(false);
    }
  }, [videoUrl, playhead]);

  const stop = () => {
    videoRef.current?.pause();
    setPlaying(false);
    setPlayhead(0);
  };
  const step = (d: number) => setPlayhead(Math.max(0, frameToTicks(ticksToFrame(playhead, fps) + d, fps)));

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      if (e.code === "Space") {
        e.preventDefault();
        togglePlay();
      } else if (e.key === "ArrowLeft" && !e.ctrlKey) step(e.shiftKey ? -10 : -1);
      else if (e.key === "ArrowRight" && !e.ctrlKey) step(e.shiftKey ? 10 : 1);
      else if (e.key === "Home") setPlayhead(0);
      else if (e.key === "End") setPlayhead(Math.max(0, duration - frameToTicks(1, fps)));
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  if (!project) return null;
  const aspect = `${project.settings.width} / ${project.settings.height}`;
  const fixedScale = zoom === "fit" ? null : Number(zoom) / 100;
  const scaleName: Record<Quality, string> = { quarter: "1/4", half: "1/2", full: "Full" };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {!compact && (
        <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line px-3 text-[11px]">
          <span className="rounded bg-raised px-1.5 py-0.5 text-muted" title="Project canvas (other ratios can be exported with reframing)">
            {project.settings.aspect}
          </span>
          <span className="tabular text-dim">
            {project.settings.width}×{project.settings.height}
          </span>
          <div className="flex-1" />
          <label className="flex items-center gap-1 text-dim" title="Preview render resolution (export always renders at full resolution)">
            Preview
            <select className="h-6 rounded border border-line px-1 text-[11px]" value={quality} onChange={(e) => setQuality(e.target.value as Quality)}>
              {(["quarter", "half", "full"] as Quality[]).map((q) => (
                <option key={q} value={q}>
                  {scaleName[q]}
                </option>
              ))}
            </select>
          </label>
          <select className="h-6 rounded border border-line px-1 text-[11px]" value={zoom} onChange={(e) => setZoom(e.target.value as "fit" | "50" | "100")} aria-label="Viewer zoom" title="Viewer zoom">
            <option value="fit">Fit</option>
            <option value="50">50%</option>
            <option value="100">100%</option>
          </select>
          <IconButton title="Safe-area guides (action / title safe, platform UI zone)" active={safe} onClick={() => setSafe(!safe)}>
            <Grid2x2 size={13} />
          </IconButton>
        </div>
      )}
      <div ref={boxRef} className={cx("relative flex min-h-0 flex-1 bg-base p-4", fixedScale ? "overflow-auto" : "items-center justify-center")}>
        {!hasClips ? (
          <EmptyState icon={<Film size={26} />} title="Nothing to preview" body="Generate an edit in the Auto Edit panel or add clips from Media." />
        ) : (
          <div
            className={cx("relative shadow-2xl ring-1 ring-line", fixedScale ? "m-auto shrink-0" : "h-full max-h-full")}
            style={fixedScale ? { width: project.settings.width * fixedScale, height: project.settings.height * fixedScale } : { aspectRatio: aspect, maxWidth: "100%" }}
          >
            {videoUrl && (
              <video
                ref={videoRef}
                src={videoUrl}
                className={cx("absolute inset-0 h-full w-full bg-black object-contain", !playing && "invisible")}
                onEnded={() => setPlaying(false)}
                onPause={() => setPlaying(false)}
              />
            )}
            {!playing && frameUrl && <img src={frameUrl} alt="Preview frame" className="absolute inset-0 h-full w-full bg-black object-contain" draggable={false} />}
            {safe && (
              <>
                <div className="pointer-events-none absolute inset-[5%] border border-dashed border-white/35" />
                <div className="pointer-events-none absolute inset-[10%] border border-dashed border-white/20" />
                <div className="pointer-events-none absolute inset-x-[5%] bottom-[5%] h-[18%] bg-white/5" />
              </>
            )}
            {loading && !playing && <Loader2 size={16} className="absolute right-2 top-2 animate-spin text-white/70" />}
          </div>
        )}
      </div>
      <div className="flex h-10 shrink-0 items-center gap-1 border-t border-line px-2">
        <IconButton title="Stop" onClick={stop}>
          <Square size={13} />
        </IconButton>
        <IconButton title="Previous frame (←)" onClick={() => step(-1)}>
          <SkipBack size={14} />
        </IconButton>
        <IconButton title={videoUrl ? "Play / pause (Space)" : "Render a preview to enable playback"} disabled={!videoUrl} onClick={togglePlay}>
          {playing ? <Pause size={15} /> : <Play size={15} />}
        </IconButton>
        <IconButton title="Next frame (→)" onClick={() => step(1)}>
          <SkipForward size={14} />
        </IconButton>
        <span className="tabular ml-2 text-xs text-muted">
          {timecode(playhead, fps)} / {timecode(duration, fps)}
        </span>
        <div className="flex-1" />
        {!videoUrl && hasClips && (
          <Button size="sm" variant="outline" disabled={rendering} onClick={renderPreview} icon={rendering ? <Loader2 size={13} className="animate-spin" /> : <Play size={13} />}>
            {rendering ? "Rendering preview…" : "Render preview for playback"}
          </Button>
        )}
        {!compact && (
          <>
            <IconButton title={muted ? "Unmute preview" : "Mute preview"} disabled={!videoUrl} onClick={() => setMuted(!muted)}>
              {muted || volume === 0 ? <VolumeX size={14} /> : <Volume2 size={14} />}
            </IconButton>
            <input
              type="range"
              aria-label="Preview volume"
              min={0}
              max={1}
              step={0.01}
              value={muted ? 0 : volume}
              disabled={!videoUrl}
              onChange={(e) => {
                setVolume(Number(e.target.value));
                setMuted(false);
              }}
              className="w-16"
            />
            <select className="ml-1 h-6 rounded border border-line px-1 text-[11px]" value={rate} disabled={!videoUrl} onChange={(e) => setRate(Number(e.target.value))} title="Playback speed (preview only)" aria-label="Playback speed">
              {[0.25, 0.5, 1, 1.5, 2].map((r) => (
                <option key={r} value={r}>
                  {r}×
                </option>
              ))}
            </select>
            <IconButton title="Fullscreen viewer" onClick={fullscreen}>
              <Fullscreen size={14} />
            </IconButton>
          </>
        )}
      </div>
    </div>
  );
}
