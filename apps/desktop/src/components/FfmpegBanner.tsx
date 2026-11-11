import { useQuery } from "@tanstack/react-query";
import { AlertTriangle } from "lucide-react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { Button } from "./ui";

/** Shown wherever FFmpeg is required but missing. Audio analysis still works without it. */
export function FfmpegBanner() {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const setRoute = useApp((s) => s.setRoute);
  if (!info.data || info.data.ffmpeg) return null;
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-warn/30 bg-warn/5 px-3 py-2">
      <div className="flex items-center gap-2 text-xs text-warn">
        <AlertTriangle size={15} />
        <span>
          FFmpeg was not found. Video import, scene analysis, preview and export need FFmpeg; music analysis works without it.
        </span>
      </div>
      <Button size="sm" variant="outline" onClick={() => setRoute("settings")}>
        Configure
      </Button>
    </div>
  );
}
