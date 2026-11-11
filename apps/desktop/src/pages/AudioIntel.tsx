import { useQuery } from "@tanstack/react-query";
import { AudioWaveform, Loader2, Pause, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, fileUrl } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import type { AudioAnalysis, WaveformPeaks } from "../types/studio";
import { Badge, Button, EmptyState, Panel, Stat, cx } from "../components/ui";
import { AudioCorrections } from "../components/AudioCorrections";

const SECTION_FILL: Record<string, string> = {
  intro: "rgba(100,116,139,0.18)",
  groove: "rgba(96,165,250,0.12)",
  build_up: "rgba(251,191,36,0.16)",
  drop: "rgba(248,113,113,0.20)",
  breakdown: "rgba(52,211,153,0.14)",
  outro: "rgba(148,163,184,0.14)",
};

export default function AudioIntel() {
  const { project, error } = useApp();
  const music = project?.assets.find((a) => a.role === "music");
  const analysis = useQuery({ queryKey: ["audio-analysis", music?.id], queryFn: api.getAudioAnalysis, enabled: !!music });
  const wave = useQuery({ queryKey: ["waveform", music?.id], queryFn: () => api.getWaveform(music!.id), enabled: !!music && !!analysis.data });
  const [busy, setBusy] = useState(false);
  const [layers, setLayers] = useState({ beats: true, kicks: true, snares: false, sections: true, bands: true });

  if (!music) return <EmptyState icon={<AudioWaveform size={26} />} title="No music imported" body="Import a track in the AI Auto Editor first." />;

  const run = async () => {
    setBusy(true);
    try {
      await waitForJob(await api.analyzeAudio());
      await analysis.refetch();
    } catch (e) {
      error(e, "Analysis failed");
    } finally {
      setBusy(false);
    }
  };

  const a = analysis.data;
  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-3">
      <Panel
        title={`Audio Intelligence — ${music.name}`}
        actions={
          <Button size="sm" disabled={busy} onClick={run} icon={busy ? <Loader2 size={13} className="animate-spin" /> : undefined}>
            {a ? "Re-run analysis" : "Analyze"}
          </Button>
        }
        className="shrink-0"
      >
        {!a ? (
          <EmptyState title="Not analyzed yet" body="Detects tempo, beats, downbeats, kicks, snares, sections and drops. Runs locally." />
        ) : (
          <div className="grid grid-cols-7 gap-4">
            <Stat label="Tempo" value={`${a.tempo.bpm.toFixed(1)} BPM`} sub={a.tempo.alternatives.slice(0, 2).map((x) => `${x.bpm.toFixed(0)}?`).join(" ")} />
            <Stat label="Beats / downbeats" value={`${a.beats.length} / ${a.beats.filter((b) => b.is_downbeat).length}`} />
            <Stat label="Kicks" value={a.kicks.length} />
            <Stat label="Snare candidates" value={a.snares.length} />
            <Stat label="Drops" value={a.drops.filter((d) => d.kind !== "false_drop_candidate").length} sub={a.drops.find((d) => d.kind === "main_drop") ? `main @ ${(a.drops.find((d) => d.kind === "main_drop")!.sample / a.sample_rate).toFixed(2)}s` : "no main drop"} />
            <Stat label="Dynamic contrast" value={`${a.summary.dynamic_contrast_db.toFixed(1)} dB`} />
            <Stat label="Rhythmic density" value={`${a.summary.rhythmic_density.toFixed(1)}/s`} />
          </div>
        )}
        {a && (
          <p className="mt-2 text-[11px] text-dim">
            Confidence values are heuristic scores (0–1), not calibrated probabilities. {a.warnings.map((w) => <span key={w} className="ml-2 text-warn">{w}</span>)}
          </p>
        )}
      </Panel>
      {a && <AudioCorrections analysis={a} />}
      {a && wave.data && (
        <Panel
          title="Analysis view"
          actions={
            <div className="flex gap-1">
              {(Object.keys(layers) as (keyof typeof layers)[]).map((k) => (
                <button key={k} className={cx("rounded px-2 py-0.5 text-[11px]", layers[k] ? "bg-overlay text-fg" : "text-dim")} onClick={() => setLayers({ ...layers, [k]: !layers[k] })}>
                  {k}
                </button>
              ))}
            </div>
          }
          bodyClass="p-0"
          className="shrink-0"
        >
          <AnalysisCanvas a={a} wave={wave.data} layers={layers} src={fileUrl(music.path)} />
        </Panel>
      )}
      {a && (
        <div className="grid shrink-0 grid-cols-2 gap-3">
          <Panel title="Sections" bodyClass="p-0">
            <table className="w-full text-xs">
              <tbody>
                {a.sections.map((s, i) => (
                  <tr key={i} className="border-b border-line">
                    <td className="px-3 py-1.5">{s.kind.replace("_", " ")}</td>
                    <td className="tabular px-2 text-dim">
                      {(s.start_sample / a.sample_rate).toFixed(2)}–{(s.end_sample / a.sample_rate).toFixed(2)}s
                    </td>
                    <td className="tabular px-2 text-dim">energy {s.energy.toFixed(2)}</td>
                    <td className="tabular px-3 text-right text-dim">conf {s.confidence.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Panel>
          <Panel title="Drop candidates" bodyClass="p-0">
            <table className="w-full text-xs">
              <tbody>
                {a.drops.map((d, i) => (
                  <tr key={i} className="border-b border-line">
                    <td className="tabular px-3 py-1.5">{(d.sample / a.sample_rate).toFixed(2)}s</td>
                    <td className="px-2">
                      <Badge tone={d.kind === "main_drop" ? "err" : d.kind === "secondary_drop" ? "warn" : "neutral"}>{d.kind.replace(/_/g, " ")}</Badge>
                    </td>
                    <td className="tabular px-3 text-right text-dim">score {d.score.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Panel>
        </div>
      )}
    </div>
  );
}

function AnalysisCanvas({ a, wave, layers, src }: { a: AudioAnalysis; wave: WaveformPeaks; layers: Record<string, boolean>; src: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [pos, setPos] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [zoom, setZoom] = useState(1);
  const dur = a.duration_samples / a.sample_rate;
  const W = Math.round(1400 * zoom);
  const H = 220;

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    cv.width = W;
    cv.height = H;
    const g = cv.getContext("2d")!;
    g.fillStyle = "#0b0b0c";
    g.fillRect(0, 0, W, H);
    const x = (sec: number) => (sec / dur) * W;
    if (layers.sections)
      for (const s of a.sections) {
        g.fillStyle = SECTION_FILL[s.kind] ?? "transparent";
        g.fillRect(x(s.start_sample / a.sample_rate), 0, x((s.end_sample - s.start_sample) / a.sample_rate), H);
        g.fillStyle = "#9a9aa2";
        g.font = "10px sans-serif";
        g.fillText(s.kind.replace("_", " "), x(s.start_sample / a.sample_rate) + 3, 11);
      }
    // waveform
    const mid = 110;
    g.fillStyle = "rgba(232,232,234,0.55)";
    const nb = wave.peaks.length / 2;
    for (let px = 0; px < W; px++) {
      const i0 = Math.floor((px / W) * nb);
      const i1 = Math.max(i0 + 1, Math.floor(((px + 1) / W) * nb));
      let lo = 0;
      let hi = 0;
      for (let i = i0; i < i1; i++) {
        lo = Math.min(lo, wave.peaks[2 * i]);
        hi = Math.max(hi, wave.peaks[2 * i + 1]);
      }
      g.fillRect(px, mid - (hi / 127) * 70, 1, Math.max(1, ((hi - lo) / 127) * 70));
    }
    // multiband energy (low / mid / high), normalized dB
    if (layers.bands) {
      const c = a.curves;
      const hopSec = c.hop_samples / a.sample_rate;
      const band = (vals: number[], color: string) => {
        g.strokeStyle = color;
        g.beginPath();
        vals.forEach((v, i) => {
          const y = H - 6 - Math.max(0, Math.min(1, (v + 60) / 60)) * 34;
          const xx = x(i * hopSec);
          if (i === 0) g.moveTo(xx, y);
          else g.lineTo(xx, y);
        });
        g.stroke();
      };
      band(c.low_db, "rgba(240,192,64,0.8)");
      band(c.mid_db, "rgba(138,180,248,0.6)");
      band(c.high_db, "rgba(192,132,252,0.5)");
    }
    if (layers.beats)
      for (const b of a.beats) {
        g.fillStyle = b.is_downbeat ? "rgba(138,180,248,0.9)" : "rgba(138,180,248,0.35)";
        g.fillRect(x(b.sample / a.sample_rate), 16, 1, b.is_downbeat ? 24 : 12);
      }
    if (layers.kicks)
      for (const k of a.kicks) {
        g.fillStyle = `rgba(240,192,64,${0.3 + 0.7 * k.confidence})`;
        g.fillRect(x(k.sample / a.sample_rate) - 1, 150, 2, 8 + 14 * k.intensity);
      }
    if (layers.snares)
      for (const k of a.snares) {
        g.fillStyle = "rgba(192,132,252,0.8)";
        g.fillRect(x(k.sample / a.sample_rate), 140, 1, 8);
      }
    for (const d of a.drops) {
      if (d.kind === "false_drop_candidate") continue;
      g.fillStyle = d.kind === "main_drop" ? "#f87171" : "#fbbf24";
      g.fillRect(x(d.sample / a.sample_rate) - 1, 0, 2, H);
    }
  }, [a, wave, layers, W, dur]);

  useEffect(() => {
    if (!playing) return;
    let r = 0;
    const t = () => {
      setPos(audioRef.current?.currentTime ?? 0);
      r = requestAnimationFrame(t);
    };
    r = requestAnimationFrame(t);
    return () => cancelAnimationFrame(r);
  }, [playing]);

  return (
    <div>
      <div className="flex items-center gap-2 border-b border-line px-3 py-1.5">
        <button
          className="text-muted hover:text-fg"
          onClick={() => {
            const el = audioRef.current!;
            if (el.paused) {
              void el.play();
              setPlaying(true);
            } else {
              el.pause();
              setPlaying(false);
            }
          }}
        >
          {playing ? <Pause size={15} /> : <Play size={15} />}
        </button>
        <span className="tabular text-xs text-muted">{pos.toFixed(2)}s</span>
        <span className="ml-auto text-[11px] text-dim">zoom</span>
        <input type="range" min={1} max={8} step={0.5} value={zoom} onChange={(e) => setZoom(parseFloat(e.target.value))} className="w-32" />
        <audio ref={audioRef} src={src} onEnded={() => setPlaying(false)} />
      </div>
      <div className="relative overflow-x-auto">
        <div className="relative" style={{ width: W }}>
          <canvas
            ref={ref}
            style={{ width: W, height: H }}
            className="block cursor-pointer"
            onClick={(e) => {
              const r = (e.target as HTMLCanvasElement).getBoundingClientRect();
              const t = ((e.clientX - r.left) / W) * dur;
              if (audioRef.current) audioRef.current.currentTime = t;
              setPos(t);
            }}
          />
          <div className="pointer-events-none absolute bottom-0 top-0 w-px bg-white" style={{ left: (pos / dur) * W }} />
        </div>
      </div>
      <div className="flex gap-4 px-3 py-1.5 text-[10px] text-dim">
        <span className="text-beat">▮ beats/downbeats</span>
        <span className="text-kick">▮ kicks</span>
        <span className="text-snare">▮ snares</span>
        <span className="text-drop">▮ main drop</span>
        <span className="text-warn">▮ secondary drop</span>
        <span>curves: low / mid / high band energy</span>
        <span className="ml-auto">Manual marker correction: planned (Phase 2)</span>
      </div>
    </div>
  );
}
