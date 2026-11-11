import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, RefreshCw } from "lucide-react";
import { api, dialogs } from "../services/api";
import { useApp } from "../stores/app";
import { Badge, Button, Panel } from "../components/ui";

export default function Settings() {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const encoders = useQuery({ queryKey: ["encoders"], queryFn: api.detectEncoders, enabled: !!info.data?.ffmpeg, staleTime: Infinity });
  const qc = useQueryClient();
  const { toast, error, jobs } = useApp();
  const pick = async () => {
    const d = await dialogs.pickFolder();
    if (!d) return;
    try {
      const t = await api.setFfmpegDir(d);
      toast({ kind: "success", title: `FFmpeg configured: ${t.version}` });
      await qc.invalidateQueries();
    } catch (e) {
      error(e, "FFmpeg not usable");
    }
  };
  const failed = Object.values(jobs).filter((j) => j.status === "failed");
  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mx-auto flex max-w-4xl flex-col gap-3">
        <Panel title="Media engine (FFmpeg)">
          {info.data?.ffmpeg ? (
            <div className="flex flex-col gap-1 text-xs">
              <div className="flex items-center gap-2"><Badge tone="ok">found</Badge> {info.data.ffmpeg.version} <span className="text-dim">({info.data.ffmpeg.source})</span></div>
              <div className="text-dim">{info.data.ffmpeg.ffmpeg}</div>
              <div className="text-dim">{info.data.ffmpeg.ffprobe}</div>
            </div>
          ) : (
            <div className="text-xs text-warn">{info.data?.ffmpeg_error ?? "Checking…"}</div>
          )}
          <div className="mt-3 flex gap-2">
            <Button icon={<FolderOpen size={13} />} onClick={pick}>Select FFmpeg folder…</Button>
            <Button variant="ghost" icon={<RefreshCw size={13} />} onClick={() => qc.invalidateQueries()}>Re-check</Button>
          </div>
          <p className="mt-2 text-[11px] text-dim">Search order: STUDIO_FFMPEG_DIR → configured folder → bundled → third_party/ffmpeg → PATH. The folder must contain ffmpeg.exe and ffprobe.exe.</p>
        </Panel>
        <Panel title="Hardware encoders" bodyClass="p-0">
          <table className="w-full text-xs">
            <tbody>
              {(encoders.data ?? []).map((e) => (
                <tr key={e.name} className="border-b border-line">
                  <td className="px-3 py-1.5 font-mono">{e.name}</td>
                  <td className="px-2 text-dim">{e.codec}</td>
                  <td className="px-2 text-dim">{e.hardware ? e.vendor : "software"}</td>
                  <td className="px-3 text-right">{e.available ? <Badge tone="ok">working</Badge> : e.listed ? <Badge tone="warn">listed, test failed</Badge> : <Badge>not in build</Badge>}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!encoders.data && <div className="p-3 text-xs text-dim">{info.data?.ffmpeg ? "Detecting…" : "Requires FFmpeg."}</div>}
        </Panel>
        <Panel title="Storage">
          <div className="grid grid-cols-1 gap-1 text-xs">
            <div><span className="text-dim">Data:</span> {info.data?.data_dir}</div>
            <div><span className="text-dim">Cache:</span> {info.data?.cache_dir}</div>
            <div><span className="text-dim">Projects:</span> {info.data?.projects_dir}</div>
            <div><span className="text-dim">CPU threads:</span> {info.data?.cpu_threads}</div>
          </div>
        </Panel>
        <Panel title="Diagnostics — failed jobs this session" bodyClass="p-0">
          {failed.length === 0 ? <div className="p-3 text-xs text-dim">No failures.</div> : failed.map((j) => (
            <details key={j.id} className="border-b border-line px-3 py-2 text-xs">
              <summary className="cursor-pointer"><span className="text-err">{j.error?.message}</span> <span className="text-dim">— {j.label}</span></summary>
              <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap text-[11px] text-dim">{j.error?.code}{"\n"}{j.error?.details}</pre>
            </details>
          ))}
        </Panel>
      </div>
    </div>
  );
}
