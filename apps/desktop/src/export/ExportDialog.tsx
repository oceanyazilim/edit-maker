import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, ChevronDown, ChevronRight, FileVideo, FolderOpen, Image as ImageIcon, Loader2, Music, X, XCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, dialogs, toStudioError } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import type { StudioError } from "../types/studio";
import { timecode, ticksToSeconds } from "../lib/time";
import { aspectSuffix, FPS_OPTIONS, fileNameError, joinPath, parseAspect, presetOf, RES_PRESETS, sizeFor, validDimension, withExt, type ResPreset } from "../lib/export";
import { Button, IconButton, inputClass, Progress, Segmented, Switch, cx } from "../components/ui";
import { tlDuration } from "../shell/commands";

type Tab = "video" | "audio" | "still";
type Quality = "standard" | "high" | "custom";
const EXTRA_ASPECTS = ["9:16", "1:1", "4:5", "16:9"];
const QUALITY: Record<Exclude<Quality, "custom">, { crf: number; speed: "quality" | "fast" }> = {
  standard: { crf: 21, speed: "fast" },
  high: { crf: 17, speed: "quality" },
};

interface Target {
  label: string;
  output: string;
  kind: Tab;
  width?: number;
  height?: number;
}
interface Outcome {
  target: Target;
  jobId?: string;
  status: "pending" | "running" | "done" | "failed" | "cancelled";
  output?: string;
  error?: StudioError;
  warnings?: string[];
}

export function ExportDialog() {
  const open = useApp((s) => s.exportOpen);
  const setOpen = useApp((s) => s.setExportOpen);
  const project = useApp((s) => s.project);
  if (!open || !project) return null;
  return <ExportDialogBody key={project.id} onClose={() => setOpen(false)} />;
}

function ExportDialogBody({ onClose }: { onClose: () => void }) {
  const project = useApp((s) => s.project)!;
  const range = useApp((s) => s.range);
  const playhead = useApp((s) => s.playhead);
  const jobs = useApp((s) => s.jobs);
  const encoders = useQuery({ queryKey: ["encoders"], queryFn: api.detectEncoders, staleTime: Infinity });
  const defaults = useQuery({ queryKey: ["export-defaults", project.id], queryFn: api.exportDefaults, staleTime: 0 });
  const settings = project.settings;
  const projAspect = parseAspect(settings.aspect) ?? [settings.width, settings.height];
  const projFps = FPS_OPTIONS.findIndex((f) => f.num * settings.fps.den === settings.fps.num * f.den);

  const [tab, setTab] = useState<Tab>("video");
  const [rangeMode, setRangeMode] = useState<"entire" | "selected">(range ? "selected" : "entire");
  const [res, setRes] = useState<ResPreset>(presetOf(settings.width, settings.height));
  const [customW, setCustomW] = useState(settings.width);
  const [customH, setCustomH] = useState(settings.height);
  const [fpsIdx, setFpsIdx] = useState(projFps >= 0 ? projFps : 2);
  const [quality, setQuality] = useState<Quality>("high");
  const [crf, setCrf] = useState(18);
  const [speed, setSpeed] = useState<"quality" | "fast" | "draft">("quality");
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [hardware, setHardware] = useState<"auto" | "gpu" | "cpu">("auto");
  const [extras, setExtras] = useState<string[]>([]);
  const [audioFmt, setAudioFmt] = useState<"m4a" | "wav">("m4a");
  const [audioKbps, setAudioKbps] = useState(192);
  const [stillFmt, setStillFmt] = useState<"png" | "jpg">("png");
  const [outcomes, setOutcomes] = useState<Outcome[] | null>(null);
  const [preflight, setPreflight] = useState<string | null>(null);
  const [frame, setFrame] = useState<string | null>(null);

  useEffect(() => {
    if (defaults.data) {
      setDir((d) => d || defaults.data!.dir);
      setName((n) => n || defaults.data!.stem);
    }
  }, [defaults.data]);

  // Poster frame at the playhead (rendered by the same compositor as the export).
  useEffect(() => {
    let url: string | null = null;
    let alive = true;
    const t = Math.min(playhead, Math.max(0, tlDuration() - 1));
    api
      .previewFrame(t, 0.4)
      .then((u) => {
        url = u;
        if (alive) setFrame(u);
        else URL.revokeObjectURL(u);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [playhead]);

  const hasGpu = !!encoders.data?.some((e) => e.hardware && e.available && e.codec === "h264");
  const shortSide = res === "custom" ? Math.min(customW, customH) : RES_PRESETS.find((r) => r.value === res)!.short!;
  const main = res === "custom" ? { width: customW, height: customH } : sizeFor(shortSide, projAspect[0], projAspect[1]);
  const fps = FPS_OPTIONS[fpsIdx];
  const useRange = rangeMode === "selected" && range;
  const durTicks = useRange ? range.end - range.start : tlDuration();
  const ext = tab === "video" ? "mp4" : tab === "audio" ? audioFmt : stillFmt;
  const fileName = withExt(name || "export", ext);
  const nameErr = fileNameError(fileName);
  const dimsErr = res === "custom" && (!validDimension(customW) || !validDimension(customH)) ? "Width and height must be even numbers between 16 and 8192." : null;
  const crfQ = quality === "custom" ? { crf, speed } : QUALITY[quality];
  const encoderLabel = hardware === "cpu" ? "libx264 (CPU)" : hardware === "gpu" ? "Hardware H.264 (GPU)" : hasGpu ? "Auto (hardware if it passes a test encode)" : "Auto (libx264, no GPU encoder found)";

  const targets: Target[] = useMemo(() => {
    const base = fileName.replace(/\.[^.]+$/, "");
    if (tab === "audio") return [{ label: `Audio (${audioFmt.toUpperCase()})`, output: joinPath(dir, fileName), kind: "audio" }];
    if (tab === "still") return [{ label: `Still frame at ${timecode(playhead, settings.fps)}`, output: joinPath(dir, fileName), kind: "still", ...main }];
    const t: Target[] = [{ label: `${settings.aspect} · ${main.width}×${main.height}`, output: joinPath(dir, fileName), kind: "video", ...main }];
    for (const a of extras) {
      const r = parseAspect(a)!;
      const sz = sizeFor(shortSide, r[0], r[1]);
      t.push({ label: `${a} reframed · ${sz.width}×${sz.height}`, output: joinPath(dir, `${base}_${aspectSuffix(a)}.mp4`), kind: "video", ...sz });
    }
    return t;
  }, [tab, fileName, dir, extras, main.width, main.height, shortSide, audioFmt, playhead, settings.aspect, settings.fps]); // eslint-disable-line react-hooks/exhaustive-deps

  const running = outcomes?.some((o) => o.status === "running" || o.status === "pending");

  const start = async () => {
    setPreflight(null);
    if (nameErr || dimsErr || !dir) return;
    if (!overwrite) {
      for (const t of targets) {
        if (await api.pathExists(t.output).catch(() => false)) {
          setPreflight(`"${t.output}" already exists. Choose another name or enable "Overwrite existing files".`);
          return;
        }
      }
    }
    const list: Outcome[] = targets.map((target) => ({ target, status: "pending" }));
    setOutcomes([...list]);
    const update = (i: number, patch: Partial<Outcome>) => {
      list[i] = { ...list[i], ...patch };
      setOutcomes([...list]);
    };
    const rs = useRange ? range.start : null;
    const re = useRange ? range.end : null;
    for (let i = 0; i < list.length; i++) {
      if (list[i].status === "cancelled") continue;
      const t = list[i].target;
      try {
        if (t.kind === "still") {
          update(i, { status: "running" });
          const out = await api.exportStill(t.output, Math.min(playhead, Math.max(0, tlDuration() - 1)), overwrite, t.width, t.height);
          update(i, { status: "done", output: out });
          continue;
        }
        const id =
          t.kind === "audio"
            ? await api.exportAudio({ output: t.output, format: audioFmt, bitrate_kbps: audioFmt === "m4a" ? audioKbps : null, overwrite, range_start: rs, range_end: re })
            : await api.exportVideo({
                output: t.output,
                encoder: hardware === "cpu" ? "libx264" : "auto",
                quality: crfQ.crf,
                speed_preset: crfQ.speed,
                audio_bitrate_kbps: 192,
                overwrite,
                range_start: rs,
                range_end: re,
                width: t.width,
                height: t.height,
                fps: { num: fps.num, den: fps.den },
                hardware,
              });
        update(i, { status: "running", jobId: id });
        const j = await waitForJob(id);
        const r = j.result as { output: string; warnings?: string[] };
        update(i, { status: "done", output: r.output, warnings: r.warnings });
      } catch (e) {
        const err = toStudioError(e);
        update(i, { status: err.code === "cancelled" ? "cancelled" : "failed", error: err });
      }
    }
    void useApp.getState().refreshProject();
  };

  const cancelAll = () => {
    for (const o of outcomes ?? []) if (o.jobId && o.status === "running") void api.cancelJob(o.jobId);
    setOutcomes((os) => os?.map((o) => (o.status === "pending" ? { ...o, status: "cancelled" } : o)) ?? null);
  };

  const summary =
    tab === "video"
      ? `${main.width}×${main.height} · ${fps.label} fps · ${timecode(durTicks, settings.fps)} · H.264 / AAC${extras.length ? ` · +${extras.length} format${extras.length > 1 ? "s" : ""}` : ""}`
      : tab === "audio"
        ? `${audioFmt === "m4a" ? `AAC ${audioKbps} kbps` : "WAV PCM 16-bit"} · ${timecode(durTicks, settings.fps)}`
        : `${stillFmt.toUpperCase()} · ${main.width}×${main.height} · frame at ${timecode(playhead, settings.fps)}`;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 p-6" onMouseDown={() => !running && onClose()}>
      <div role="dialog" aria-modal="true" aria-label="Export" className="flex max-h-full w-[920px] max-w-full flex-col overflow-hidden rounded-xl border border-line bg-surface shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <header className="flex h-11 shrink-0 items-center justify-between border-b border-line px-4">
          <h2 className="text-sm font-semibold">Export</h2>
          <IconButton title={running ? "Export running — cancel it first" : "Close"} disabled={running} onClick={onClose}>
            <X size={15} />
          </IconButton>
        </header>

        {outcomes ? (
          <ProgressView outcomes={outcomes} jobs={jobs} running={!!running} onCancel={cancelAll} onBack={() => setOutcomes(null)} onClose={onClose} />
        ) : (
          <>
            <div className="flex min-h-0 flex-1">
              {/* Left: poster + facts */}
              <div className="flex w-[330px] shrink-0 flex-col gap-3 border-r border-line bg-base/40 p-4">
                <div className="flex h-[300px] items-center justify-center rounded-lg bg-black/40">
                  {frame ? (
                    <img src={frame} alt="Frame at the playhead" className="max-h-full max-w-full rounded object-contain shadow-lg" style={{ aspectRatio: `${settings.width} / ${settings.height}` }} />
                  ) : (
                    <Loader2 size={18} className="animate-spin text-dim" />
                  )}
                </div>
                <dl className="grid grid-cols-[92px_1fr] gap-y-1.5 text-[11px]">
                  <dt className="text-dim">Project</dt>
                  <dd className="truncate">{project.name}</dd>
                  <dt className="text-dim">Canvas</dt>
                  <dd>
                    {settings.width}×{settings.height} · {settings.aspect}
                  </dd>
                  <dt className="text-dim">Duration</dt>
                  <dd className="tabular">
                    {timecode(durTicks, settings.fps)} {useRange && <span className="text-dim">(selection)</span>}
                  </dd>
                  <dt className="text-dim">Encoder</dt>
                  <dd className="truncate" title={encoderLabel}>
                    {tab === "video" ? encoderLabel : tab === "audio" ? (audioFmt === "m4a" ? "AAC" : "PCM") : stillFmt.toUpperCase()}
                  </dd>
                  <dt className="text-dim">Files</dt>
                  <dd>{targets.length}</dd>
                </dl>
                <p className="mt-auto text-[10px] leading-relaxed text-dim">Rendered by the same engine as the preview, written to a temporary file, verified with ffprobe, then moved into place. Cancelling removes the partial file.</p>
              </div>

              {/* Right: settings */}
              <div className="min-w-0 flex-1 overflow-y-auto p-4">
                <Segmented
                  value={tab}
                  onChange={setTab}
                  className="mb-4"
                  options={[
                    { value: "video", label: <span className="inline-flex items-center gap-1.5"><FileVideo size={13} /> Video</span> },
                    { value: "audio", label: <span className="inline-flex items-center gap-1.5"><Music size={13} /> Audio Only</span> },
                    { value: "still", label: <span className="inline-flex items-center gap-1.5"><ImageIcon size={13} /> Still Frame</span> },
                  ]}
                />
                <div className="flex flex-col gap-4">
                  {tab !== "still" && (
                    <Row label="Range">
                      <Segmented
                        size="sm"
                        value={rangeMode}
                        onChange={setRangeMode}
                        options={[
                          { value: "entire", label: `Entire (${ticksToSeconds(tlDuration()).toFixed(2)}s)` },
                          { value: "selected", label: range ? `Selected (${ticksToSeconds(range.start).toFixed(2)}–${ticksToSeconds(range.end).toFixed(2)}s)` : "Selected", disabled: !range, title: range ? undefined : "Shift+drag on the timeline ruler to select a range" },
                        ]}
                      />
                    </Row>
                  )}
                  {tab !== "audio" && (
                    <Row label="Resolution" hint={res === "custom" ? undefined : `${main.width}×${main.height} for ${settings.aspect}`}>
                      <Segmented size="sm" value={res} onChange={setRes} options={RES_PRESETS.map((r) => ({ value: r.value, label: r.label }))} />
                      {res === "custom" && (
                        <div className="mt-2 flex items-center gap-2">
                          <input className={inputClass + " h-7 w-24"} type="number" step={2} min={16} max={8192} value={customW} onChange={(e) => setCustomW(Math.round(Number(e.target.value)))} aria-label="Width" />
                          <span className="text-dim">×</span>
                          <input className={inputClass + " h-7 w-24"} type="number" step={2} min={16} max={8192} value={customH} onChange={(e) => setCustomH(Math.round(Number(e.target.value)))} aria-label="Height" />
                          <span className="text-[10px] text-dim">a different ratio is reframed (cover crop)</span>
                        </div>
                      )}
                      {dimsErr && <div className="mt-1 text-[11px] text-err">{dimsErr}</div>}
                    </Row>
                  )}
                  {tab === "video" && (
                    <>
                      <Row label="Frame rate">
                        <select className={inputClass + " h-7 w-40"} value={fpsIdx} onChange={(e) => setFpsIdx(Number(e.target.value))}>
                          {FPS_OPTIONS.map((f, i) => (
                            <option key={f.label} value={i}>
                              {f.label} fps{i === projFps ? " (project)" : ""}
                            </option>
                          ))}
                        </select>
                      </Row>
                      <Row label="Quality">
                        <Segmented
                          size="sm"
                          value={quality}
                          onChange={setQuality}
                          options={[
                            { value: "standard", label: "Standard", title: "CRF 21, fast preset" },
                            { value: "high", label: "High", title: "CRF 17, quality preset" },
                            { value: "custom", label: "Custom" },
                          ]}
                        />
                        {quality === "custom" && (
                          <div className="mt-2 flex items-center gap-3">
                            <label className="flex items-center gap-1.5 text-[11px] text-muted">
                              CRF
                              <input className={inputClass + " h-7 w-16"} type="number" min={0} max={51} value={crf} onChange={(e) => setCrf(Math.max(0, Math.min(51, Math.round(Number(e.target.value)))))} />
                            </label>
                            <label className="flex items-center gap-1.5 text-[11px] text-muted">
                              Preset
                              <select className={inputClass + " h-7"} value={speed} onChange={(e) => setSpeed(e.target.value as typeof speed)}>
                                <option value="quality">Quality</option>
                                <option value="fast">Fast</option>
                                <option value="draft">Draft</option>
                              </select>
                            </label>
                            <span className="text-[10px] text-dim">lower CRF = better / larger</span>
                          </div>
                        )}
                      </Row>
                    </>
                  )}
                  {tab === "audio" && (
                    <Row label="Format">
                      <div className="flex items-center gap-3">
                        <Segmented size="sm" value={audioFmt} onChange={setAudioFmt} options={[{ value: "m4a", label: "M4A (AAC)" }, { value: "wav", label: "WAV" }]} />
                        {audioFmt === "m4a" && (
                          <select className={inputClass + " h-7"} value={audioKbps} onChange={(e) => setAudioKbps(Number(e.target.value))} aria-label="Bitrate">
                            {[128, 192, 256, 320].map((b) => (
                              <option key={b} value={b}>
                                {b} kbps
                              </option>
                            ))}
                          </select>
                        )}
                      </div>
                    </Row>
                  )}
                  {tab === "still" && (
                    <Row label="Format" hint={`Frame at the playhead (${timecode(playhead, settings.fps)})`}>
                      <Segmented size="sm" value={stillFmt} onChange={setStillFmt} options={[{ value: "png", label: "PNG" }, { value: "jpg", label: "JPG" }]} />
                    </Row>
                  )}

                  <Row label="File name">
                    <div className="flex items-center gap-1">
                      <input className={cx(inputClass, "h-7 min-w-0 flex-1", nameErr && "border-err")} value={name} onChange={(e) => setName(e.target.value)} placeholder="export" spellCheck={false} />
                      <span className="text-[11px] text-dim">.{ext}</span>
                    </div>
                    {nameErr && <div className="mt-1 text-[11px] text-err">{nameErr}</div>}
                  </Row>
                  <Row label="Folder">
                    <div className="flex items-center gap-1">
                      <input className={inputClass + " h-7 min-w-0 flex-1 text-[11px]"} readOnly value={dir} title={dir} placeholder="Choose a folder…" />
                      <Button size="sm" icon={<FolderOpen size={12} />} onClick={async () => setDir((await dialogs.pickFolder()) ?? dir)}>
                        Browse
                      </Button>
                    </div>
                    <label className="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted">
                      <input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} /> Overwrite existing files
                    </label>
                  </Row>

                  {tab === "video" && (
                    <div className="rounded-lg border border-line">
                      <button className="flex h-8 w-full items-center gap-1 px-3 text-[11px] font-semibold uppercase tracking-wider text-muted hover:text-fg" onClick={() => setAdvanced(!advanced)} aria-expanded={advanced}>
                        {advanced ? <ChevronDown size={12} /> : <ChevronRight size={12} />} Advanced
                      </button>
                      {advanced && (
                        <div className="flex flex-col gap-3 border-t border-line p-3">
                          <Row label="Hardware">
                            <Segmented
                              size="sm"
                              value={hardware}
                              onChange={setHardware}
                              options={[
                                { value: "auto", label: "Auto" },
                                { value: "gpu", label: "GPU", disabled: !hasGpu, title: hasGpu ? "Hardware H.264 encoder" : encoders.isLoading ? "Detecting encoders…" : "No hardware H.264 encoder passed a test encode on this machine" },
                                { value: "cpu", label: "CPU", title: "libx264 software encoder" },
                              ]}
                            />
                          </Row>
                          <Row label="Also export" hint="Each extra format is a separate MP4 reframed around each clip's focus point (no stretching).">
                            <div className="flex flex-wrap gap-1.5">
                              {EXTRA_ASPECTS.filter((a) => a !== settings.aspect).map((a) => (
                                <label key={a} className={cx("flex h-7 cursor-pointer items-center gap-1.5 rounded-md border px-2 text-[11px]", extras.includes(a) ? "border-accent bg-accent/10" : "border-line hover:bg-raised")}>
                                  <input type="checkbox" checked={extras.includes(a)} onChange={(e) => setExtras(e.target.checked ? [...extras, a] : extras.filter((x) => x !== a))} />
                                  {a}
                                </label>
                              ))}
                            </div>
                          </Row>
                          <div className="flex flex-col gap-2 rounded-md bg-base/50 p-2.5">
                            <Switch checked={false} onChange={() => undefined} disabled label="AI enhancement" hint="Not implemented" />
                            <Switch checked={false} onChange={() => undefined} disabled label="Smooth motion (frame interpolation)" hint="Not implemented" />
                            <Switch checked={false} onChange={() => undefined} disabled label="Detail recovery / upscaling" hint="Not implemented" />
                            <span className="text-[10px] text-dim">These need real processing that the engine does not have yet, so they are disabled.</span>
                          </div>
                        </div>
                      )}
                    </div>
                  )}
                  {preflight && (
                    <div className="flex gap-2 rounded-md border border-warn/30 bg-warn/10 p-2 text-[11px] text-warn">
                      <AlertTriangle size={13} className="mt-px shrink-0" /> {preflight}
                    </div>
                  )}
                </div>
              </div>
            </div>
            <footer className="flex shrink-0 items-center gap-3 border-t border-line px-4 py-3">
              <div className="tabular min-w-0 flex-1 truncate text-[11px] text-muted" title={summary}>
                {summary}
              </div>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" disabled={!!nameErr || !!dimsErr || !dir || durTicks <= 0} onClick={start}>
                Export{targets.length > 1 ? ` ${targets.length} files` : ""}
              </Button>
            </footer>
          </>
        )}
      </div>
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[96px_1fr] items-start gap-3">
      <div className="pt-1.5 text-[11px] font-medium text-muted">{label}</div>
      <div className="min-w-0">
        {children}
        {hint && <div className="mt-1 text-[10px] text-dim">{hint}</div>}
      </div>
    </div>
  );
}

function ProgressView({
  outcomes,
  jobs,
  running,
  onCancel,
  onBack,
  onClose,
}: {
  outcomes: Outcome[];
  jobs: ReturnType<typeof useApp.getState>["jobs"];
  running: boolean;
  onCancel: () => void;
  onBack: () => void;
  onClose: () => void;
}) {
  const error = useApp((s) => s.error);
  const open = (p: string, reveal: boolean) => api.openPath(p, reveal).catch((e) => error(e, reveal ? "Could not open the folder" : "Could not open the file"));
  const done = outcomes.filter((o) => o.status === "done").length;
  return (
    <>
      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        <div className="mb-4 flex items-center gap-2 text-sm">
          {running ? <Loader2 size={16} className="animate-spin text-accent" /> : done === outcomes.length ? <CheckCircle2 size={16} className="text-ok" /> : <AlertTriangle size={16} className="text-warn" />}
          {running ? "Exporting…" : `${done} of ${outcomes.length} file${outcomes.length > 1 ? "s" : ""} exported`}
        </div>
        <div className="flex flex-col gap-2">
          {outcomes.map((o, i) => {
            const j = o.jobId ? jobs[o.jobId] : undefined;
            return (
              <div key={i} className="rounded-lg border border-line bg-raised p-3">
                <div className="flex items-center gap-2">
                  {o.status === "done" ? (
                    <CheckCircle2 size={14} className="shrink-0 text-ok" />
                  ) : o.status === "failed" ? (
                    <XCircle size={14} className="shrink-0 text-err" />
                  ) : o.status === "cancelled" ? (
                    <XCircle size={14} className="shrink-0 text-dim" />
                  ) : o.status === "running" ? (
                    <Loader2 size={14} className="shrink-0 animate-spin text-accent" />
                  ) : (
                    <span className="h-3.5 w-3.5 shrink-0 rounded-full border border-line-strong" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="text-xs font-medium">{o.target.label}</div>
                    <div className="truncate text-[10px] text-dim" title={o.output ?? o.target.output}>
                      {o.output ?? o.target.output}
                    </div>
                  </div>
                  {o.status === "done" && o.output && (
                    <div className="flex shrink-0 gap-1">
                      <Button size="sm" onClick={() => open(o.output!, true)} icon={<FolderOpen size={12} />}>
                        Open Folder
                      </Button>
                      <Button size="sm" variant="primary" onClick={() => open(o.output!, false)}>
                        Open File
                      </Button>
                    </div>
                  )}
                </div>
                {o.status === "running" && j && (
                  <div className="mt-2">
                    <Progress value={j.status === "queued" ? null : j.progress} />
                    <div className="tabular mt-1 flex justify-between text-[10px] text-dim">
                      <span>{j.stage}</span>
                      <span>{j.progress != null ? `${Math.round(j.progress * 100)}%` : ""}</span>
                    </div>
                  </div>
                )}
                {o.error && (
                  <details className="mt-2 text-[11px] text-err">
                    <summary className="cursor-pointer">{o.error.message}</summary>
                    {o.error.details && <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-[10px] text-dim">{o.error.details}</pre>}
                  </details>
                )}
                {o.warnings && o.warnings.length > 0 && <div className="mt-1.5 text-[10px] text-warn">{o.warnings.join(" · ")}</div>}
              </div>
            );
          })}
        </div>
      </div>
      <footer className="flex shrink-0 justify-end gap-2 border-t border-line px-4 py-3">
        {running ? (
          <Button variant="danger" onClick={onCancel}>
            Cancel export
          </Button>
        ) : (
          <>
            <Button onClick={onBack}>Back to settings</Button>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </>
        )}
      </footer>
    </>
  );
}
