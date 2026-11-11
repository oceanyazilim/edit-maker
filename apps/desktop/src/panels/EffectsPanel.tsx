import { Plus, Sparkles } from "lucide-react";
import { useApp } from "../stores/app";
import { Badge, PanelHeader } from "../components/ui";
import { EFFECTS, withEffect } from "../editor/effects";
import { selectedClip } from "../shell/commands";

/** Add compositor effects to the selected clip; parameters are edited in the Inspector. */
export function EffectsPanel() {
  useApp((s) => s.project);
  const selectedId = useApp((s) => s.selectedClipId);
  const playhead = useApp((s) => s.playhead);
  const editTimeline = useApp((s) => s.editTimeline);
  const sel = selectedId ? selectedClip() : null;
  const clip = sel?.track.kind === "video" ? sel.clip : null;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Effects" actions={clip && <Badge>{clip.effects.length} on clip</Badge>} />
      <div className="border-b border-line px-3 py-2 text-[11px] text-dim">
        {clip ? "Click an effect to add it at the playhead. Edit its parameters in the Inspector." : "Select a video clip on the timeline to add effects."}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        <div className="flex flex-col gap-1">
          {EFFECTS.map((d) => (
            <button
              key={d.type}
              disabled={!clip}
              onClick={() => clip && editTimeline({ op: "update_clip", clip: withEffect(clip, d, playhead) })}
              className="group flex items-start gap-2 rounded-md border border-line bg-raised p-2 text-left hover:border-line-strong disabled:cursor-not-allowed disabled:opacity-45"
            >
              <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded bg-accent/15 text-accent">
                <Sparkles size={12} />
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5 text-[11px] font-medium">
                  {d.label}
                  {d.wholeClip && <span className="text-[9px] font-normal text-dim">whole clip</span>}
                </span>
                <span className="block text-[10px] leading-snug text-dim">{d.description}</span>
              </span>
              <Plus size={13} className="mt-1 shrink-0 text-dim group-hover:text-accent" />
            </button>
          ))}
        </div>
        <p className="mt-3 px-1 text-[10px] leading-relaxed text-dim">Kick- and drop-triggered effects are added automatically by Auto Edit; use Sequence → Reapply Effects to recompute them.</p>
      </div>
    </div>
  );
}
