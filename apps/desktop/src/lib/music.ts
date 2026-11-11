import { useQuery } from "@tanstack/react-query";
import { api } from "../services/api";
import type { AudioAnalysis, Clip, Project, Tick } from "../types/studio";
import { secondsToTicks, ticksToSeconds } from "./time";

/** The music asset and its timeline clip (first clip on an audio track that uses it). */
export function musicOf(project: Project | null | undefined) {
  const asset = project?.assets.find((a) => a.role === "music") ?? null;
  let clip: Clip | null = null;
  if (project && asset) {
    for (const t of project.timeline.tracks) {
      if (t.kind !== "audio") continue;
      const c = t.clips.find((c) => c.asset_id === asset.id);
      if (c) {
        clip = c;
        break;
      }
    }
  }
  return { asset, clip };
}

/** Music time (seconds in the song) → timeline tick, or null when outside the used part. */
export function songToTimeline(sec: number, clip: Clip | null, clampOutside = false): Tick | null {
  if (!clip) return null;
  const t = clip.start + (secondsToTicks(sec) - clip.source_in) / (clip.speed || 1);
  if (!clampOutside && (t < clip.start || t > clip.start + clip.duration)) return null;
  return Math.round(t);
}

/** Timeline tick → music time in seconds (null without a music clip). */
export function timelineToSong(t: Tick, clip: Clip | null): number | null {
  if (!clip) return null;
  return ticksToSeconds(clip.source_in + (t - clip.start) * (clip.speed || 1));
}

export const sampleSec = (a: AudioAnalysis, sample: number) => sample / a.sample_rate;

/** Shared queries: music asset, corrected analysis (cached only) and waveform peaks. */
export function useMusic(project: Project | null | undefined) {
  const { asset, clip } = musicOf(project);
  const analysis = useQuery({ queryKey: ["audio-analysis", asset?.id], queryFn: api.getAudioAnalysis, enabled: !!asset });
  const wave = useQuery({ queryKey: ["waveform", asset?.id], queryFn: () => api.getWaveform(asset!.id), enabled: !!asset && !asset.missing, staleTime: Infinity });
  return { asset, clip, analysis: analysis.data ?? null, analysisLoading: analysis.isLoading, wave: wave.data ?? null };
}
