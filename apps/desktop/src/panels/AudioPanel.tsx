import { AudioLines, Volume2, VolumeX } from "lucide-react";
import { useApp } from "../stores/app";
import { secondsToTicks, ticksToSeconds } from "../lib/time";
import { useMusic } from "../lib/music";
import { Button, EmptyState, PanelHeader, Section, Slider, Stat } from "../components/ui";
import { AudioCorrections } from "../components/AudioCorrections";
import { cmd } from "../shell/commands";

export function AudioPanel() {
  const project = useApp((s) => s.project);
  const editTimeline = useApp((s) => s.editTimeline);
  const setRoute = useApp((s) => s.setRoute);
  const { asset, clip, analysis } = useMusic(project);
  if (!project) return null;
  const track = clip ? project.timeline.tracks.find((t) => t.clips.some((c) => c.id === clip.id)) : null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Audio" actions={<Button size="sm" variant="ghost" onClick={() => setRoute("audio")}>Full analysis</Button>} />
      {!asset ? (
        <EmptyState icon={<AudioLines size={22} />} title="No music" body="Import the project's music first." action={<Button onClick={cmd.importMusic}>Import music</Button>} />
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <Section title="Music clip">
            {clip && track ? (
              <div className="flex flex-col gap-3">
                <div className="flex items-center justify-between text-[11px] text-muted">
                  <span className="truncate">{asset.name}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={track.muted ? <VolumeX size={12} /> : <Volume2 size={12} />}
                    onClick={() => editTimeline({ op: "set_track_flags", track_id: track.id, muted: !track.muted })}
                  >
                    {track.muted ? "Muted" : "Mute"}
                  </Button>
                </div>
                <Slider label="Volume" min={0} max={2} value={clip.volume} format={(v) => `${(20 * Math.log10(Math.max(v, 1e-4))).toFixed(1)} dB`} onChange={(v) => editTimeline({ op: "update_clip", clip: { ...clip, volume: v } })} />
                <Slider label="Fade in" min={0} max={Math.min(5, ticksToSeconds(clip.duration) / 2)} value={ticksToSeconds(clip.fade_in)} format={(v) => `${v.toFixed(2)}s`} onChange={(v) => editTimeline({ op: "update_clip", clip: { ...clip, fade_in: secondsToTicks(v) } })} />
                <Slider label="Fade out" min={0} max={Math.min(5, ticksToSeconds(clip.duration) / 2)} value={ticksToSeconds(clip.fade_out)} format={(v) => `${v.toFixed(2)}s`} onChange={(v) => editTimeline({ op: "update_clip", clip: { ...clip, fade_out: secondsToTicks(v) } })} />
              </div>
            ) : (
              <p className="text-[11px] text-dim">The music is placed on the timeline when an edit is generated.</p>
            )}
          </Section>
          {analysis ? (
            <>
              <Section title="Detected">
                <div className="grid grid-cols-3 gap-2">
                  <Stat label="Tempo" value={`${analysis.tempo.bpm.toFixed(1)}`} sub="BPM" />
                  <Stat label="Kicks" value={analysis.kicks.length} />
                  <Stat label="Drops" value={analysis.drops.filter((d) => d.kind !== "false_drop_candidate").length} />
                </div>
              </Section>
              <div className="p-2">
                <AudioCorrections analysis={analysis} compact />
              </div>
            </>
          ) : (
            <p className="p-3 text-[11px] text-dim">Analyze the music in the Auto Edit panel to see tempo, kicks and drops and to correct them.</p>
          )}
        </div>
      )}
    </div>
  );
}
