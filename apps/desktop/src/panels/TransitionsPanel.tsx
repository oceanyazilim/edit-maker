import { Shuffle } from "lucide-react";
import { useApp } from "../stores/app";
import type { TransitionKind } from "../types/studio";
import { secondsToTicks, ticksToSeconds } from "../lib/time";
import { EmptyState, PanelHeader, Slider, cx } from "../components/ui";
import { TRANSITIONS } from "../editor/effects";
import { selectedClip } from "../shell/commands";

/** Transition into the selected clip (rendered by the compositor). */
export function TransitionsPanel() {
  useApp((s) => s.project);
  const selectedId = useApp((s) => s.selectedClipId);
  const editTimeline = useApp((s) => s.editTimeline);
  const sel = selectedId ? selectedClip() : null;
  const isVideo = sel?.track.kind === "video";
  const clip = sel?.clip;
  const current = clip?.transition_in?.kind ?? "cut";
  const apply = (v: TransitionKind | "cut") => {
    if (!clip) return;
    void editTimeline({ op: "update_clip", clip: { ...clip, transition_in: v === "cut" ? null : { kind: v, duration: clip.transition_in?.duration ?? secondsToTicks(0.3) } } });
  };
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Transitions" />
      {!clip || !isVideo ? (
        <EmptyState icon={<Shuffle size={22} />} title="Select a video clip" body="The transition plays into the selected clip from the one before it." />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          <div className="mb-2 text-[11px] text-dim">Into the selected clip</div>
          <div className="grid grid-cols-2 gap-1.5">
            {TRANSITIONS.map((t) => (
              <button
                key={t.value}
                onClick={() => apply(t.value)}
                className={cx("rounded-md border p-2 text-left", current === t.value ? "border-accent bg-accent/10" : "border-line bg-raised hover:border-line-strong")}
              >
                <TransitionGlyph kind={t.value} />
                <div className="mt-1.5 text-[11px] font-medium">{t.label}</div>
                <div className="text-[10px] leading-snug text-dim">{t.description}</div>
              </button>
            ))}
          </div>
          {clip.transition_in && (
            <div className="mt-4">
              <Slider
                label="Duration"
                min={0.06}
                max={Math.max(0.1, Math.min(2, ticksToSeconds(clip.duration)))}
                value={ticksToSeconds(clip.transition_in.duration)}
                format={(v) => `${v.toFixed(2)}s`}
                onChange={(v) => editTimeline({ op: "update_clip", clip: { ...clip, transition_in: { ...clip.transition_in!, duration: secondsToTicks(v) } } })}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function TransitionGlyph({ kind }: { kind: string }) {
  const a = "#3a4a63";
  const b = "#5b6f8f";
  return (
    <svg viewBox="0 0 60 22" className="h-5 w-full" aria-hidden>
      <defs>
        <linearGradient id={`g-${kind}`} x1="0" x2="1">
          <stop offset="0" stopColor={a} />
          <stop offset="1" stopColor={b} />
        </linearGradient>
      </defs>
      {kind === "cut" && (
        <>
          <rect x="0" y="2" width="29" height="18" rx="2" fill={a} />
          <rect x="31" y="2" width="29" height="18" rx="2" fill={b} />
        </>
      )}
      {kind === "cross_dissolve" && <rect x="0" y="2" width="60" height="18" rx="2" fill={`url(#g-${kind})`} />}
      {kind === "flash_cut" && (
        <>
          <rect x="0" y="2" width="60" height="18" rx="2" fill={`url(#g-${kind})`} />
          <rect x="26" y="2" width="8" height="18" fill="#fff" opacity="0.9" />
        </>
      )}
      {kind === "zoom_blend" && (
        <>
          <rect x="0" y="2" width="60" height="18" rx="2" fill={`url(#g-${kind})`} />
          <rect x="22" y="6" width="16" height="10" rx="1" fill="none" stroke="#fff" opacity="0.6" />
        </>
      )}
      {kind === "dip_to_black" && (
        <>
          <rect x="0" y="2" width="60" height="18" rx="2" fill={`url(#g-${kind})`} />
          <rect x="22" y="2" width="16" height="18" fill="#000" />
        </>
      )}
    </svg>
  );
}
