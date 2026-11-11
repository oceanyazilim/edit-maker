import { Lock, Plus, RotateCcw, SlidersHorizontal, Trash2, Unlock } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { fileUrl } from "../services/api";
import { useApp } from "../stores/app";
import type { Clip, EffectInstance, EffectKind, TransitionKind } from "../types/studio";
import { fmtSeconds, secondsToTicks, ticksToSeconds } from "../lib/time";
import { Badge, Button, EmptyState, Field, IconButton, Section, Segmented, Select, Slider, Tabs, cx } from "../components/ui";
import { EFFECTS, PARAMS, TRANSITIONS, withEffect } from "./effects";

type Tab = "video" | "speed" | "animate" | "adjust";
const ADJUST = "adjust";

export function Inspector() {
  const project = useApp((s) => s.project);
  const selectedClipId = useApp((s) => s.selectedClipId);
  const [tab, setTab] = useState<Tab>("video");
  const found = useMemo(() => {
    for (const t of project?.timeline.tracks ?? []) {
      const c = t.clips.find((c) => c.id === selectedClipId);
      if (c) return { clip: c, track: t };
    }
    return null;
  }, [project, selectedClipId]);
  const isVideo = found?.track.kind === "video";
  useEffect(() => {
    if (found && !isVideo && tab !== "video" && tab !== "speed") setTab("video");
  }, [found, isVideo, tab]);

  if (!project) return null;
  const tabs = [
    { value: "video" as const, label: isVideo || !found ? "Video" : "Audio" },
    { value: "speed" as const, label: "Speed" },
    { value: "animate" as const, label: "Animate", disabled: !!found && !isVideo, title: !isVideo && found ? "Only video clips can be animated" : undefined },
    { value: "adjust" as const, label: "Adjust", disabled: !!found && !isVideo, title: !isVideo && found ? "Only video clips have colour adjustments" : undefined },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Tabs value={tab} onChange={setTab} tabs={tabs} />
      {!found ? (
        <EmptyState icon={<SlidersHorizontal size={22} />} title="No clip selected" body="Select a clip on the timeline to edit its framing, speed, animation and colour." />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ClipHeader clip={found.clip} trackLocked={found.track.locked} />
          {tab === "video" && (isVideo ? <VideoTab clip={found.clip} /> : <AudioTab clip={found.clip} />)}
          {tab === "speed" && <SpeedTab clip={found.clip} isVideo={isVideo} />}
          {tab === "animate" && isVideo && <AnimateTab clip={found.clip} />}
          {tab === "adjust" && isVideo && <AdjustTab clip={found.clip} />}
        </div>
      )}
      {found && found.track.locked && <div className="border-t border-line px-3 py-1.5 text-[10px] text-warn">Track {found.track.name} is locked — changes are rejected.</div>}
    </div>
  );
}

const useUpdate = () => {
  const editTimeline = useApp((s) => s.editTimeline);
  return (c: Clip) => editTimeline({ op: "update_clip", clip: c });
};

function ClipHeader({ clip, trackLocked }: { clip: Clip; trackLocked: boolean }) {
  const project = useApp((s) => s.project)!;
  const editTimeline = useApp((s) => s.editTimeline);
  const asset = project.assets.find((a) => a.id === clip.asset_id);
  return (
    <div className="border-b border-line p-3">
      <div className="flex gap-2.5">
        {asset?.thumbnail ? <img src={fileUrl(asset.thumbnail)} className="h-10 w-16 shrink-0 rounded object-cover" alt="" /> : <div className="h-10 w-16 shrink-0 rounded bg-raised" />}
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-medium" title={asset?.path}>
            {asset?.name ?? "Missing media"}
          </div>
          <div className="tabular mt-0.5 text-[10px] text-dim">
            {fmtSeconds(ticksToSeconds(clip.start))} → {fmtSeconds(ticksToSeconds(clip.start + clip.duration))} · {fmtSeconds(ticksToSeconds(clip.duration))}
          </div>
          <div className="tabular text-[10px] text-dim">
            source in {fmtSeconds(ticksToSeconds(clip.source_in))} · {clip.speed.toFixed(2)}×
          </div>
        </div>
        <IconButton
          title={clip.locked ? "Unlock clip (L) — regeneration may change it" : "Lock clip (L) — kept exactly on regenerate"}
          active={clip.locked}
          disabled={trackLocked}
          onClick={() => editTimeline({ op: "set_clip_locked", clip_id: clip.id, locked: !clip.locked })}
        >
          {clip.locked ? <Lock size={13} className="text-accent" /> : <Unlock size={13} />}
        </IconButton>
      </div>
      {clip.origin.reason && (
        <div className="mt-2 rounded-md bg-raised px-2 py-1.5 text-[10px] leading-snug text-muted">
          <Badge tone="accent">planner</Badge> <span className="ml-1">{clip.origin.reason}</span>
          {clip.origin.score != null && <span className="ml-1 text-dim">score {clip.origin.score.toFixed(2)}</span>}
        </div>
      )}
    </div>
  );
}

function VideoTab({ clip }: { clip: Clip }) {
  const update = useUpdate();
  const playhead = useApp((s) => s.playhead);
  const setFx = (id: string, e: EffectInstance) => update({ ...clip, effects: clip.effects.map((x) => (x.id === id ? e : x)) });
  const fx = clip.effects.filter((e) => e.trigger !== ADJUST);
  return (
    <>
      <Section title="Framing">
        <div className="flex flex-col gap-2.5">
          <Segmented
            size="sm"
            value={clip.framing.mode}
            onChange={(v) => update({ ...clip, framing: { ...clip.framing, mode: v } })}
            options={[
              { value: "cover", label: "Fill (crop)" },
              { value: "fit", label: "Fit (letterbox)" },
            ]}
          />
          <Slider label="Position X" value={clip.framing.center_x} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => update({ ...clip, framing: { ...clip.framing, center_x: v } })} />
          <Slider label="Position Y" value={clip.framing.center_y} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => update({ ...clip, framing: { ...clip.framing, center_y: v } })} />
          <Slider label="Scale" min={0.5} max={3} value={clip.framing.zoom} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => update({ ...clip, framing: { ...clip.framing, zoom: v } })} />
          <Slider label="Rotation" min={-180} max={180} step={1} value={clip.framing.rotation_deg} format={(v) => `${v.toFixed(0)}°`} onChange={(v) => update({ ...clip, framing: { ...clip.framing, rotation_deg: v } })} />
          <Button size="sm" variant="ghost" className="self-start" icon={<RotateCcw size={11} />} onClick={() => update({ ...clip, framing: { ...clip.framing, center_x: 0.5, center_y: 0.5, zoom: 1, rotation_deg: 0 } })}>
            Reset framing
          </Button>
        </div>
      </Section>
      <Section title="Transition in">
        <div className="flex flex-col gap-2.5">
          <Select<string>
            value={clip.transition_in?.kind ?? "cut"}
            onChange={(v) => update({ ...clip, transition_in: v === "cut" ? null : { kind: v as TransitionKind, duration: clip.transition_in?.duration ?? secondsToTicks(0.3) } })}
            options={TRANSITIONS.map((t) => ({ value: t.value, label: t.label }))}
          />
          {clip.transition_in && (
            <Slider label="Duration" min={0.06} max={2} value={ticksToSeconds(clip.transition_in.duration)} format={(v) => `${v.toFixed(2)}s`} onChange={(v) => update({ ...clip, transition_in: { ...clip.transition_in!, duration: secondsToTicks(v) } })} />
          )}
        </div>
      </Section>
      <Section
        title={`Effects (${fx.length})`}
        actions={
          <select
            className="h-6 rounded border border-line px-1 text-[10px]"
            value=""
            aria-label="Add effect"
            onChange={(e) => {
              const def = EFFECTS.find((d) => d.type === e.target.value);
              if (def) update(withEffect(clip, def, playhead));
            }}
          >
            <option value="">+ Add…</option>
            {EFFECTS.filter((d) => !d.wholeClip).map((d) => (
              <option key={d.type} value={d.type}>
                {d.label}
              </option>
            ))}
          </select>
        }
      >
        {fx.length === 0 && <p className="text-[11px] text-dim">No effects. Add one here or from the Effects panel.</p>}
        <div className="flex flex-col gap-2">
          {fx.map((e) => (
            <div key={e.id} className="rounded-md border border-line bg-raised p-2">
              <div className="mb-1.5 flex items-center justify-between">
                <span className="text-[11px] font-medium capitalize">
                  {e.kind.type.replace(/_/g, " ")} {e.trigger && <span className="text-[9px] font-normal text-dim">{e.trigger}</span>}
                </span>
                <IconButton title="Remove effect" onClick={() => update({ ...clip, effects: clip.effects.filter((x) => x.id !== e.id) })}>
                  <Trash2 size={11} />
                </IconButton>
              </div>
              <div className="flex flex-col gap-2">
                {(PARAMS[e.kind.type] ?? []).map((p) => (
                  <Slider
                    key={p.key}
                    label={p.label}
                    min={p.min}
                    max={p.max}
                    step={p.step}
                    value={(e.kind as unknown as Record<string, number>)[p.key]}
                    onChange={(v) => setFx(e.id, { ...e, kind: { ...e.kind, [p.key]: v } as EffectKind })}
                  />
                ))}
                <div className="tabular grid grid-cols-2 text-[10px] text-dim">
                  <span>at +{fmtSeconds(ticksToSeconds(e.offset))}</span>
                  <span>for {fmtSeconds(ticksToSeconds(e.duration))}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </Section>
    </>
  );
}

function AudioTab({ clip }: { clip: Clip }) {
  const update = useUpdate();
  const maxFade = Math.min(5, ticksToSeconds(clip.duration) / 2);
  return (
    <Section title="Audio">
      <div className="flex flex-col gap-2.5">
        <Slider label="Volume" min={0} max={2} value={clip.volume} format={(v) => `${(20 * Math.log10(Math.max(v, 1e-4))).toFixed(1)} dB`} onChange={(v) => update({ ...clip, volume: v })} />
        <Slider label="Fade in" min={0} max={maxFade} value={ticksToSeconds(clip.fade_in)} format={(v) => `${v.toFixed(2)}s`} onChange={(v) => update({ ...clip, fade_in: secondsToTicks(v) })} />
        <Slider label="Fade out" min={0} max={maxFade} value={ticksToSeconds(clip.fade_out)} format={(v) => `${v.toFixed(2)}s`} onChange={(v) => update({ ...clip, fade_out: secondsToTicks(v) })} />
      </div>
    </Section>
  );
}

function SpeedTab({ clip, isVideo }: { clip: Clip; isVideo: boolean }) {
  const update = useUpdate();
  const project = useApp((s) => s.project)!;
  const asset = project.assets.find((a) => a.id === clip.asset_id);
  const isMusic = asset?.role === "music";
  const srcDur = asset && asset.kind !== "image" ? ticksToSeconds(asset.metadata.duration) : null;
  const avail = srcDur != null ? srcDur - ticksToSeconds(clip.source_in) : null;
  const maxSpeed = avail != null ? Math.max(0.1, Math.min(4, avail / Math.max(1e-6, ticksToSeconds(clip.duration)))) : 4;
  const used = ticksToSeconds(clip.duration) * clip.speed;
  const set = (v: number) => update({ ...clip, speed: Math.round(v * 100) / 100 });
  if (isMusic)
    return (
      <Section title="Speed">
        <p className="text-[11px] leading-relaxed text-dim">The music plays at its original speed — the beat grid and every cut depend on it.</p>
      </Section>
    );
  return (
    <>
      <Section title="Constant speed">
        <div className="flex flex-col gap-2.5">
          <Slider label="Speed" min={0.25} max={4} step={0.05} value={clip.speed} format={(v) => `${v.toFixed(2)}×`} onChange={set} />
          <div className="grid grid-cols-5 gap-1">
            {[0.5, 0.75, 1, 1.5, 2].map((v) => (
              <button
                key={v}
                disabled={v > maxSpeed + 1e-6}
                title={v > maxSpeed + 1e-6 ? "Not enough source media after the in-point" : undefined}
                onClick={() => set(v)}
                className={cx("h-6 rounded text-[10px] disabled:opacity-30", Math.abs(clip.speed - v) < 0.001 ? "bg-accent text-white" : "bg-raised text-muted hover:text-fg")}
              >
                {v}×
              </button>
            ))}
          </div>
          <div className="tabular text-[10px] leading-relaxed text-dim">
            Uses {used.toFixed(2)}s of source{avail != null && ` · ${avail.toFixed(2)}s available after the in-point (max ${maxSpeed.toFixed(2)}×)`}.
            {isVideo && " Clip length on the timeline stays the same."}
          </div>
        </div>
      </Section>
      <Section title="Speed curve">
        <p className="text-[11px] leading-relaxed text-dim">Speed curves are not implemented yet. Auto Edit builds velocity ramps by splitting a shot into constant-speed parts, which you can adjust individually here.</p>
      </Section>
    </>
  );
}

const EASES = [
  { value: "linear", label: "Linear" },
  { value: "ease_in_out", label: "Ease in-out" },
  { value: "ease_out", label: "Ease out" },
  { value: "hold", label: "Hold" },
];

function AnimateTab({ clip }: { clip: Clip }) {
  const update = useUpdate();
  const playhead = useApp((s) => s.playhead);
  const setPlayhead = useApp((s) => s.setPlayhead);
  const local = playhead - clip.start;
  const inside = local >= 0 && local <= clip.duration;
  const kfs = [...clip.keyframes].sort((a, b) => a.t - b.t);
  const setKfs = (k: Clip["keyframes"]) => update({ ...clip, keyframes: [...k].sort((a, b) => a.t - b.t) });
  const interp = () => {
    // Value at the playhead so a new keyframe doesn't jump.
    if (!kfs.length) return { zoom: 1, offset_x: 0, offset_y: 0 };
    if (local <= kfs[0].t) return kfs[0];
    for (let i = 0; i + 1 < kfs.length; i++) {
      const a = kfs[i];
      const b = kfs[i + 1];
      if (local >= a.t && local < b.t) {
        const x = (local - a.t) / Math.max(1, b.t - a.t);
        return { zoom: a.zoom + (b.zoom - a.zoom) * x, offset_x: a.offset_x + (b.offset_x - a.offset_x) * x, offset_y: a.offset_y + (b.offset_y - a.offset_y) * x };
      }
    }
    return kfs[kfs.length - 1];
  };
  const add = () => {
    if (!inside) return;
    const v = interp();
    setKfs([...kfs.filter((k) => Math.abs(k.t - local) > secondsToTicks(0.01)), { t: local, zoom: v.zoom, offset_x: v.offset_x, offset_y: v.offset_y, ease: "ease_in_out" }]);
  };
  const preset = (kind: "zoom_in" | "zoom_out" | "pan") => {
    const end = clip.duration;
    const k =
      kind === "zoom_in"
        ? [
            { t: 0, zoom: 1, offset_x: 0, offset_y: 0, ease: "linear" },
            { t: end, zoom: 1.15, offset_x: 0, offset_y: 0, ease: "ease_in_out" },
          ]
        : kind === "zoom_out"
          ? [
              { t: 0, zoom: 1.15, offset_x: 0, offset_y: 0, ease: "linear" },
              { t: end, zoom: 1, offset_x: 0, offset_y: 0, ease: "ease_in_out" },
            ]
          : [
              { t: 0, zoom: 1.12, offset_x: -0.04, offset_y: 0, ease: "linear" },
              { t: end, zoom: 1.12, offset_x: 0.04, offset_y: 0, ease: "ease_in_out" },
            ];
    setKfs(k);
  };
  return (
    <>
      <Section title="Presets">
        <div className="grid grid-cols-3 gap-1">
          <Button size="sm" onClick={() => preset("zoom_in")}>
            Zoom in
          </Button>
          <Button size="sm" onClick={() => preset("zoom_out")}>
            Zoom out
          </Button>
          <Button size="sm" onClick={() => preset("pan")}>
            Pan
          </Button>
        </div>
        <p className="mt-1.5 text-[10px] text-dim">Replaces this clip's keyframes. Combined with Scale from the Video tab.</p>
      </Section>
      <Section
        title={`Keyframes (${kfs.length})`}
        actions={
          <Button size="sm" variant="ghost" disabled={!inside} title={inside ? "Add a keyframe at the playhead" : "Move the playhead inside this clip"} icon={<Plus size={11} />} onClick={add}>
            At playhead
          </Button>
        }
      >
        {kfs.length === 0 && <p className="text-[11px] text-dim">No keyframes. Add one at the playhead or use a preset.</p>}
        <div className="flex flex-col gap-2">
          {kfs.map((k, i) => (
            <div key={`${k.t}-${i}`} className="rounded-md border border-line bg-raised p-2">
              <div className="mb-1.5 flex items-center justify-between">
                <button className="tabular text-[11px] font-medium hover:text-accent" title="Go to keyframe" onClick={() => setPlayhead(clip.start + k.t)}>
                  +{ticksToSeconds(k.t).toFixed(2)}s
                </button>
                <div className="flex items-center gap-1">
                  <select className="h-6 rounded border border-line px-1 text-[10px]" value={k.ease} onChange={(e) => setKfs(kfs.map((x, j) => (j === i ? { ...x, ease: e.target.value } : x)))} title="Easing into this keyframe">
                    {EASES.map((e) => (
                      <option key={e.value} value={e.value}>
                        {e.label}
                      </option>
                    ))}
                  </select>
                  <IconButton title="Delete keyframe" onClick={() => setKfs(kfs.filter((_, j) => j !== i))}>
                    <Trash2 size={11} />
                  </IconButton>
                </div>
              </div>
              <div className="flex flex-col gap-2">
                <Slider label="Zoom" min={0.5} max={3} value={k.zoom} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => setKfs(kfs.map((x, j) => (j === i ? { ...x, zoom: v } : x)))} />
                <Slider label="Offset X" min={-0.5} max={0.5} step={0.005} value={k.offset_x} onChange={(v) => setKfs(kfs.map((x, j) => (j === i ? { ...x, offset_x: v } : x)))} />
                <Slider label="Offset Y" min={-0.5} max={0.5} step={0.005} value={k.offset_y} onChange={(v) => setKfs(kfs.map((x, j) => (j === i ? { ...x, offset_y: v } : x)))} />
              </div>
            </div>
          ))}
        </div>
      </Section>
    </>
  );
}

function AdjustTab({ clip }: { clip: Clip }) {
  const update = useUpdate();
  const color = clip.effects.find((e) => e.trigger === ADJUST && e.kind.type === "color");
  const vig = clip.effects.find((e) => e.trigger === ADJUST && e.kind.type === "vignette");
  const c = (color?.kind ?? { type: "color", brightness: 0, contrast: 1, saturation: 1 }) as Extract<EffectKind, { type: "color" }>;
  const vStrength = vig ? (vig.kind as Extract<EffectKind, { type: "vignette" }>).strength : 0;
  const upsert = (kind: EffectKind, existing?: EffectInstance) => {
    const fx: EffectInstance = existing ? { ...existing, kind, offset: 0, duration: clip.duration } : { id: `fx_adjust_${kind.type}_${Date.now()}`, kind, offset: 0, duration: clip.duration, version: 1, trigger: ADJUST };
    update({ ...clip, effects: existing ? clip.effects.map((e) => (e.id === existing.id ? fx : e)) : [...clip.effects, fx] });
  };
  const autoColor = clip.effects.filter((e) => e.trigger !== ADJUST && (e.kind.type === "color" || e.kind.type === "vignette"));
  return (
    <>
      <Section
        title="Color"
        actions={
          color && (
            <Button size="sm" variant="ghost" icon={<RotateCcw size={11} />} onClick={() => update({ ...clip, effects: clip.effects.filter((e) => e.id !== color.id) })}>
              Reset
            </Button>
          )
        }
      >
        <div className="flex flex-col gap-2.5">
          <Slider label="Brightness" min={-0.5} max={0.5} value={c.brightness} format={(v) => (v >= 0 ? `+${v.toFixed(2)}` : v.toFixed(2))} onChange={(v) => upsert({ ...c, brightness: v }, color)} />
          <Slider label="Contrast" min={0.5} max={2} value={c.contrast} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => upsert({ ...c, contrast: v }, color)} />
          <Slider label="Saturation" min={0} max={2} value={c.saturation} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => upsert({ ...c, saturation: v }, color)} />
        </div>
      </Section>
      <Section
        title="Vignette"
        actions={
          vig && (
            <Button size="sm" variant="ghost" icon={<RotateCcw size={11} />} onClick={() => update({ ...clip, effects: clip.effects.filter((e) => e.id !== vig.id) })}>
              Reset
            </Button>
          )
        }
      >
        <Slider label="Strength" min={0} max={1} value={vStrength} onChange={(v) => upsert({ type: "vignette", strength: v }, vig)} />
      </Section>
      {autoColor.length > 0 && <p className="px-3 py-2 text-[10px] leading-relaxed text-dim">This clip also has {autoColor.length} colour/vignette effect(s) from the style preset; they are applied on top. Edit them in the Video tab.</p>}
      <Section title="Enhance" defaultOpen={false}>
        <Field label="Sharpen / denoise / HDR">
          <p className="text-[11px] text-dim">Not implemented yet.</p>
        </Field>
      </Section>
    </>
  );
}
