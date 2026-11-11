import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, Dice5, Film, Loader2, Music, RefreshCcw, Sparkles, Star, Upload, Wand2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, waitForJobSafe } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import type { AudioAnalysis, EditPreset, ProjectSettings, WaveformPeaks } from "../types/studio";
import { fmtDate, fmtSeconds, ticksToSeconds } from "../lib/time";
import { songToTimeline, timelineToSong, useMusic } from "../lib/music";
import { Badge, Button, EmptyState, Field, IconButton, inputClass, PanelHeader, Progress, Section, Segmented, Select, cx } from "../components/ui";
import { ControlSliders } from "../components/ControlSliders";
import { VARIANTS } from "../components/NewProjectDialog";
import { SegmentPanel } from "../components/SegmentPanel";
import { CuePanel } from "../components/CuePanel";
import { runRegen } from "../components/RegenTools";
import { cmd } from "../shell/commands";

const SECTION_FILL: Record<string, string> = {
  intro: "#2b3240",
  groove: "#2c3a4c",
  build_up: "#4a3c22",
  drop: "#5a2a2e",
  breakdown: "#24403b",
  outro: "#2e2e38",
};

export function AutoEditPanel() {
  const project = useApp((s) => s.project);
  const patchProject = useApp((s) => s.patchProject);
  const error = useApp((s) => s.error);
  const toast = useApp((s) => s.toast);
  const jobs = useApp((s) => s.jobs);
  const setLeftTool = useApp((s) => s.setLeftTool);
  const qc = useQueryClient();
  const { asset: music, clip: musicClip, analysis, wave } = useMusic(project);
  const [genJob, setGenJob] = useState<string | null>(null);
  const [analyzing, setAnalyzing] = useState(false);

  if (!project) return null;
  const settings = project.settings;
  const footage = project.assets.filter((a) => a.role === "footage");
  const gen = genJob ? jobs[genJob] : undefined;
  const generating = !!gen && (gen.status === "running" || gen.status === "queued");

  const persist = async (s: ProjectSettings) => {
    try {
      patchProject(await api.updateSettings(s));
    } catch (e) {
      error(e, "Setting not saved");
    }
  };

  const analyze = async () => {
    setAnalyzing(true);
    try {
      await waitForJobSafe(await api.analyzeAudio());
      await qc.invalidateQueries({ queryKey: ["audio-analysis"] });
      await qc.invalidateQueries({ queryKey: ["segments"] });
    } catch (e) {
      error(e, "Analysis failed");
    } finally {
      setAnalyzing(false);
    }
  };

  const generate = async (seed?: number) => {
    try {
      if (seed != null) patchProject(await api.updateSettings({ ...settings, seed }));
      const id = await api.generateEdit();
      setGenJob(id);
      const j = await waitForJob(id);
      await useApp.getState().refreshProject();
      const r = j.result as { clips: number; warnings: string[] };
      toast({ kind: "success", title: `Edit generated: ${r.clips} shots`, details: r.warnings.join("\n") || undefined });
    } catch (e) {
      error(e, "Generation failed");
    }
  };

  const mainDrop = analysis?.drops.find((d) => d.kind === "main_drop") ?? analysis?.drops.find((d) => d.kind !== "false_drop_candidate");
  // The part of the song the edit uses: the actual music clip when an edit exists, else the chosen start.
  const usedStart = musicClip ? ticksToSeconds(musicClip.source_in) : settings.audio_start_secs ?? null;
  const usedEnd = musicClip ? ticksToSeconds(musicClip.source_in + musicClip.duration * (musicClip.speed || 1)) : usedStart != null ? usedStart + settings.duration_secs : null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Auto Edit" actions={<Badge tone={project.plan ? "ok" : "neutral"}>{project.plan ? `${project.plan.segments.length} shots` : "not generated"}</Badge>} />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* ---- Music ---- */}
        <div className="border-b border-line p-3">
          {music ? (
            <>
              <div className="flex items-start gap-2">
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-accent/15 text-accent">
                  <Music size={15} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-xs font-medium" title={music.path}>
                    {music.name}
                  </div>
                  <div className="text-[11px] text-dim">
                    {fmtSeconds(ticksToSeconds(music.metadata.duration), 1)} · {music.metadata.audio[0]?.sample_rate ?? "?"} Hz
                    {music.missing && <span className="ml-1 text-err">missing</span>}
                  </div>
                </div>
                <IconButton title="Replace music" onClick={cmd.importMusic}>
                  <Upload size={13} />
                </IconButton>
              </div>
              <div className="mt-2.5 grid grid-cols-3 gap-1.5">
                <MiniStat label="BPM" value={analysis ? analysis.tempo.bpm.toFixed(1) : "—"} sub={analysis ? (analysis.tempo.overridden ? "manual" : `conf ${analysis.tempo.confidence.toFixed(2)}`) : undefined} />
                <MiniStat label="Main drop" value={analysis && mainDrop ? `${(mainDrop.sample / analysis.sample_rate).toFixed(2)}s` : analysis ? "none" : "—"} accent="drop" />
                <MiniStat label="Range" value={usedStart != null && usedEnd != null ? `${usedStart.toFixed(1)}–${usedEnd.toFixed(1)}` : "auto"} sub={usedStart != null ? "seconds" : "planner picks"} />
              </div>
              {analysis ? (
                wave && <MiniWaveform analysis={analysis} wave={wave} usedStart={usedStart} usedEnd={usedEnd} settings={settings} onSettings={persist} />
              ) : (
                <Button className="mt-2.5 w-full" size="sm" disabled={analyzing || music.missing} onClick={analyze} icon={analyzing ? <Loader2 size={12} className="animate-spin" /> : <Sparkles size={12} />}>
                  {analyzing ? "Analyzing beats, kicks and drops…" : "Analyze music"}
                </Button>
              )}
              {analysis && analysis.warnings.length > 0 && (
                <div className="mt-2 flex gap-1 text-[10px] text-warn" title={analysis.warnings.join("\n")}>
                  <AlertTriangle size={11} className="mt-px shrink-0" /> <span className="truncate">{analysis.warnings[0]}</span>
                </div>
              )}
            </>
          ) : (
            <EmptyState icon={<Music size={20} />} title="No music" body="Import the track for this project. The edit is cut to its beats." action={<Button size="sm" variant="primary" onClick={cmd.importMusic}>Import music</Button>} />
          )}
        </div>

        {/* ---- Footage ---- */}
        <div className="flex items-center justify-between border-b border-line px-3 py-2">
          <span className="flex items-center gap-1.5 text-xs text-muted">
            <Film size={13} /> {footage.length} clip{footage.length === 1 ? "" : "s"} in project
          </span>
          <Button size="sm" variant="ghost" onClick={() => setLeftTool("media")}>
            Manage media
          </Button>
        </div>

        {/* ---- Style ---- */}
        <StyleGrid value={settings.preset_id} onChange={(id) => persist({ ...settings, preset_id: id })} />

        {/* ---- Edit setup ---- */}
        <Section title="Edit setup">
          <div className="flex flex-col gap-2.5">
            <div className="grid grid-cols-2 gap-2">
              <Field label="Variant">
                <Select value={settings.variant} onChange={(v) => persist({ ...settings, variant: v })} options={VARIANTS} />
              </Field>
              <Field label="Seed">
                <div className="flex gap-1">
                  <input className={inputClass + " w-full min-w-0"} type="number" value={settings.seed} onChange={(e) => persist({ ...settings, seed: Math.max(0, Number(e.target.value) || 0) })} />
                  <IconButton title="New random seed" onClick={() => persist({ ...settings, seed: Math.floor(Math.random() * 1_000_000) + 1 })}>
                    <Dice5 size={14} />
                  </IconButton>
                </div>
              </Field>
            </div>
            <p className="text-[10px] text-dim">Same media + settings + seed = identical edit.</p>
          </div>
        </Section>
        <div className="flex flex-col gap-2 border-b border-line p-2">
          <SegmentPanel settings={settings} hasMusic={!!music} analyzed={!!analysis} onChange={persist} />
          <CuePanel cues={settings.cues ?? []} footage={footage} onChange={(cues) => persist({ ...settings, cues })} />
        </div>
        <Section title="Creative controls" defaultOpen={false}>
          <ControlSliders cols={1} value={settings.controls} onChange={(c) => persist({ ...settings, controls: c })} />
        </Section>
        <Section title={`Versions (${project.versions?.length ?? 0})`} defaultOpen={false}>
          <Versions />
        </Section>
      </div>

      {/* ---- Generate ---- */}
      <div className="shrink-0 border-t border-line bg-surface p-3">
        {generating && (
          <div className="mb-2">
            <div className="mb-1 truncate text-[11px] text-muted">{gen?.stage ?? "Generating…"}</div>
            <Progress value={gen?.progress} />
          </div>
        )}
        <Button variant="primary" className="h-9 w-full" disabled={!music || footage.length === 0 || generating} onClick={() => generate()} icon={generating ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}>
          {generating ? "Generating…" : project.plan ? "Regenerate edit" : "Generate edit"}
        </Button>
        {(!music || footage.length === 0) && <p className="mt-1.5 text-[10px] text-dim">{!music ? "Import music to enable generation." : "Import footage to enable generation."}</p>}
        {project.plan && !generating && (
          <>
            <div className="mt-2 grid grid-cols-3 gap-1">
              <Button size="sm" title="Keep locked clips, regenerate the rest" onClick={() => runRegen({ mode: "keep_locked" })} icon={<RefreshCcw size={11} />}>
                Unlocked
              </Button>
              <Button size="sm" title="Keep clips and timing, recompute effects" onClick={() => runRegen({ mode: "effects_only" })} icon={<Sparkles size={11} />}>
                Effects
              </Button>
              <Button size="sm" title="New random seed, then generate" onClick={() => generate(Math.floor(Math.random() * 1_000_000) + 1)} icon={<Dice5 size={11} />}>
                Variation
              </Button>
            </div>
            <div className="mt-2 flex items-center gap-1 text-[10px] text-dim">
              <CheckCircle2 size={11} className="text-ok" /> seed {project.plan.seed} · {project.plan.variant.replace("_", " ")}
              {project.plan.warnings.length > 0 && (
                <span className="ml-auto text-warn" title={project.plan.warnings.join("\n")}>
                  {project.plan.warnings.length} warning{project.plan.warnings.length > 1 ? "s" : ""}
                </span>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function MiniStat({ label, value, sub, accent }: { label: string; value: string; sub?: string; accent?: "drop" }) {
  return (
    <div className="rounded-md border border-line bg-raised px-2 py-1.5">
      <div className="text-[10px] text-dim">{label}</div>
      <div className={cx("tabular truncate text-[13px] font-semibold", accent === "drop" && value !== "none" && value !== "—" && "text-drop")}>{value}</div>
      {sub && <div className="truncate text-[9px] text-dim">{sub}</div>}
    </div>
  );
}

/**
 * Whole-song waveform with sections, downbeats, drops, the used range and the playhead.
 * Click seeks inside the edit; Alt+click sets the music start at the nearest bar.
 */
function MiniWaveform({
  analysis,
  wave,
  usedStart,
  usedEnd,
  settings,
  onSettings,
}: {
  analysis: AudioAnalysis;
  wave: WaveformPeaks;
  usedStart: number | null;
  usedEnd: number | null;
  settings: ProjectSettings;
  onSettings: (s: ProjectSettings) => void;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const project = useApp((s) => s.project);
  const playhead = useApp((s) => s.playhead);
  const setPlayhead = useApp((s) => s.setPlayhead);
  const toast = useApp((s) => s.toast);
  const [w, setW] = useState(280);
  const [hover, setHover] = useState<number | null>(null);
  const { clip } = useMusic(project);
  const total = analysis.duration_samples / analysis.sample_rate;
  const H = 64;

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(Math.max(100, Math.floor(el.clientWidth))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = window.devicePixelRatio || 1;
    cv.width = w * dpr;
    cv.height = H * dpr;
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, H);
    const x = (sec: number) => (sec / total) * w;
    // Section bands (top strip).
    for (const s of analysis.sections) {
      ctx.fillStyle = SECTION_FILL[s.kind] ?? "#2a2a33";
      ctx.fillRect(x(s.start_sample / analysis.sample_rate), 0, Math.max(1, x((s.end_sample - s.start_sample) / analysis.sample_rate)), 6);
    }
    // Used range.
    if (usedStart != null && usedEnd != null) {
      ctx.fillStyle = "rgba(67,135,239,0.16)";
      ctx.fillRect(x(usedStart), 6, Math.max(1, x(usedEnd) - x(usedStart)), H - 6);
    }
    // Waveform.
    const nb = Math.floor(wave.peaks.length / 2);
    const mid = 6 + (H - 6) / 2;
    const amp = (H - 10) / 2;
    for (let px = 0; px < w; px++) {
      const i0 = Math.floor((px / w) * nb);
      const i1 = Math.max(i0 + 1, Math.floor(((px + 1) / w) * nb));
      let lo = 0;
      let hi = 0;
      for (let i = i0; i < i1 && 2 * i + 1 < wave.peaks.length; i++) {
        lo = Math.min(lo, wave.peaks[2 * i]);
        hi = Math.max(hi, wave.peaks[2 * i + 1]);
      }
      const sec = (px / w) * total;
      const inside = usedStart != null && usedEnd != null && sec >= usedStart && sec <= usedEnd;
      ctx.fillStyle = inside ? "rgba(122,167,245,0.95)" : "rgba(151,155,165,0.45)";
      const y0 = mid - (hi / 127) * amp;
      const y1 = mid - (lo / 127) * amp;
      ctx.fillRect(px, y0, 1, Math.max(1, y1 - y0));
    }
    // Downbeats (every bar) as faint ticks along the bottom.
    ctx.fillStyle = "rgba(231,233,237,0.18)";
    for (const b of analysis.beats) if (b.is_downbeat) ctx.fillRect(Math.round(x(b.sample / analysis.sample_rate)), H - 4, 1, 4);
    // Drops.
    for (const d of analysis.drops) {
      if (d.kind === "false_drop_candidate") continue;
      ctx.fillStyle = d.kind === "main_drop" ? "#f26d6d" : "rgba(242,109,109,0.55)";
      ctx.fillRect(Math.round(x(d.sample / analysis.sample_rate)), 6, d.kind === "main_drop" ? 2 : 1, H - 6);
    }
  }, [analysis, wave, w, total, usedStart, usedEnd]);

  const songPh = timelineToSong(playhead, clip);
  const secAt = (clientX: number) => {
    const r = boxRef.current!.getBoundingClientRect();
    return Math.max(0, Math.min(total, ((clientX - r.left) / r.width) * total));
  };
  const nearestBar = (sec: number) => {
    let best = 0;
    let bd = Infinity;
    for (const b of analysis.beats) {
      if (!b.is_downbeat) continue;
      const t = b.sample / analysis.sample_rate;
      if (Math.abs(t - sec) < bd) {
        bd = Math.abs(t - sec);
        best = t;
      }
    }
    return bd === Infinity ? sec : best;
  };

  return (
    <div className="mt-2.5">
      <div
        ref={boxRef}
        className="relative h-16 cursor-pointer overflow-hidden rounded-md border border-line bg-base"
        onMouseMove={(e) => setHover(secAt(e.clientX))}
        onMouseLeave={() => setHover(null)}
        onClick={(e) => {
          const sec = secAt(e.clientX);
          if (e.altKey) {
            const start = Math.max(0, Math.min(nearestBar(sec), total - settings.duration_secs));
            onSettings({ ...settings, audio_start_secs: Math.round(start * 1000) / 1000 });
            toast({ kind: "info", title: `Music start set to ${start.toFixed(2)}s (bar)`, details: "Regenerate the edit to apply it." });
            return;
          }
          const t = songToTimeline(sec, clip);
          if (t == null) {
            toast({ kind: "info", title: clip ? "That part of the song is not in the edit" : "Generate an edit to seek", details: "Alt+click sets the music start at the nearest bar." });
            return;
          }
          setPlayhead(t);
        }}
        title="Click: seek · Alt+click: start the edit at this bar"
      >
        <canvas ref={ref} style={{ width: w, height: H }} className="block" />
        {songPh != null && songPh >= 0 && songPh <= total && <div className="pointer-events-none absolute inset-y-0 w-px bg-white" style={{ left: `${(songPh / total) * 100}%` }} />}
        {hover != null && (
          <div className="pointer-events-none absolute inset-y-0 w-px bg-white/30" style={{ left: `${(hover / total) * 100}%` }}>
            <span className="tabular absolute -top-px left-1 rounded bg-black/70 px-1 text-[9px] text-white/90">{hover.toFixed(2)}s</span>
          </div>
        )}
      </div>
      <div className="mt-1 flex items-center justify-between text-[9px] text-dim">
        <span className="flex items-center gap-2">
          <Legend color="#f26d6d" label="drop" />
          <Legend color="rgba(67,135,239,0.6)" label="in edit" />
          <Legend color="#4a3c22" label="build" />
        </span>
        <span>Alt+click: set start</span>
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <span className="h-2 w-2 rounded-sm" style={{ background: color }} />
      {label}
    </span>
  );
}

/** Style grid from the preset engine: built-in presets and the user's own. */
function StyleGrid({ value, onChange }: { value: string; onChange: (id: string) => void }) {
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  const favs = useQuery({ queryKey: ["favorite-presets"], queryFn: api.favoritePresets });
  const qc = useQueryClient();
  const setRoute = useApp((s) => s.setRoute);
  const [tab, setTab] = useState<"builtin" | "mine">("builtin");
  const list = (presets.data ?? []).filter((p) => (tab === "builtin" ? p.builtin : !p.builtin));
  const fav = new Set(favs.data ?? []);
  const sorted = [...list].sort((a, b) => Number(fav.has(b.id)) - Number(fav.has(a.id)) || a.name.localeCompare(b.name));
  const toggleFav = async (p: EditPreset) => {
    try {
      await api.setFavoritePreset(p.id, !fav.has(p.id));
      await qc.invalidateQueries({ queryKey: ["favorite-presets"] });
    } catch (e) {
      useApp.getState().error(e);
    }
  };
  return (
    <Section title="Style">
      <Segmented
        size="sm"
        className="mb-2"
        value={tab}
        onChange={setTab}
        options={[
          { value: "builtin", label: "Built-in" },
          { value: "mine", label: `My Styles${presets.data ? ` (${presets.data.filter((p) => !p.builtin).length})` : ""}` },
        ]}
      />
      {presets.isLoading && <Progress value={null} />}
      {sorted.length === 0 && !presets.isLoading && (
        <div className="rounded-md border border-dashed border-line p-3 text-center text-[11px] text-dim">
          {tab === "mine" ? (
            <>
              No custom styles yet.{" "}
              <button className="text-accent hover:underline" onClick={() => setRoute("presets")}>
                Create one in Effects & Presets
              </button>
            </>
          ) : (
            "No built-in presets found."
          )}
        </div>
      )}
      <div className="grid grid-cols-2 gap-1.5">
        {sorted.map((p) => {
          const sel = p.id === value;
          return (
            <div
              key={p.id}
              role="button"
              tabIndex={0}
              aria-pressed={sel}
              onClick={() => onChange(p.id)}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onChange(p.id)}
              title={p.description}
              className={cx(
                "group relative cursor-pointer rounded-md border p-2 text-left transition-colors",
                sel ? "border-accent bg-accent/10" : "border-line bg-raised hover:border-line-strong",
              )}
            >
              <div className="flex items-start justify-between gap-1">
                <span className={cx("line-clamp-1 text-[11px] font-medium", sel && "text-fg")}>{p.name}</span>
                <button
                  className={cx("shrink-0", fav.has(p.id) ? "text-kick" : "text-dim opacity-0 group-hover:opacity-100")}
                  title={fav.has(p.id) ? "Remove from favorites" : "Add to favorites"}
                  onClick={(e) => {
                    e.stopPropagation();
                    void toggleFav(p);
                  }}
                >
                  <Star size={11} fill={fav.has(p.id) ? "currentColor" : "none"} />
                </button>
              </div>
              <div className="mt-0.5 line-clamp-2 text-[10px] leading-snug text-dim">{p.description}</div>
              <PacingBar preset={p} />
            </div>
          );
        })}
      </div>
    </Section>
  );
}

/** Tiny visual of a preset's real parameters: cut pace, effect intensity, drop impact. */
function PacingBar({ preset }: { preset: EditPreset }) {
  const pace = Math.max(0, Math.min(1, 1 - (preset.pacing.base_shot_beats - 0.5) / 7.5));
  const fx = Math.max(0, Math.min(1, preset.effects.intensity));
  const drop = Math.max(0, Math.min(1, preset.drop.impact));
  const bar = (v: number, c: string, label: string) => (
    <div className="flex-1" title={`${label} ${(v * 100).toFixed(0)}%`}>
      <div className="h-0.5 rounded bg-line">
        <div className="h-full rounded" style={{ width: `${v * 100}%`, background: c }} />
      </div>
    </div>
  );
  return (
    <div className="mt-1.5 flex gap-1">
      {bar(pace, "#4387ef", "Cut pace")}
      {bar(fx, "#f2c14b", "Effects")}
      {bar(drop, "#f26d6d", "Drop impact")}
    </div>
  );
}

function Versions() {
  const project = useApp((s) => s.project);
  if (!project?.versions?.length) return <p className="text-[11px] text-dim">Each generate / regenerate saves the previous timeline here.</p>;
  return (
    <div className="flex flex-col gap-1">
      {[...project.versions].reverse().map((v) => (
        <div key={v.id} className="flex items-center justify-between gap-2 rounded-md px-1.5 py-1 hover:bg-raised">
          <div className="min-w-0">
            <div className="truncate text-[11px]">{v.label}</div>
            <div className="text-[10px] text-dim">
              {fmtDate(v.created_at)} · {v.timeline.tracks[0]?.clips.length ?? 0} clips · seed {v.seed}
            </div>
          </div>
          <Button
            size="sm"
            title="Restore this version (the current timeline is saved first)"
            onClick={async () => {
              try {
                useApp.getState().setProject(await api.restoreVersion(v.id));
                useApp.getState().setDirty(true);
              } catch (e) {
                useApp.getState().error(e, "Restore failed");
              }
            }}
          >
            Restore
          </Button>
        </div>
      ))}
    </div>
  );
}

export { SECTION_FILL };
