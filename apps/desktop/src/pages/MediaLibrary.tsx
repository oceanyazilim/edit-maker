import { useQuery } from "@tanstack/react-query";
import { Link2, Loader2, ScanSearch, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { api, dialogs, fileUrl } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import type { MediaAsset } from "../types/studio";
import { fmtBytes, fmtSeconds, ticksToSeconds } from "../lib/time";
import { Badge, Button, EmptyState, inputClass, Panel, cx } from "../components/ui";

export default function MediaLibrary() {
  const { project, patchProject: setProject, error } = useApp();
  const [sel, setSel] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const assets = useMemo(() => (project?.assets ?? []).filter((a) => a.name.toLowerCase().includes(q.toLowerCase())), [project, q]);
  if (!project) return <EmptyState title="No project open" />;
  const asset = project.assets.find((a) => a.id === sel) ?? assets[0];

  return (
    <div className="flex h-full gap-3 p-3">
      <Panel title={`Media (${project.assets.length})`} className="w-96 shrink-0" bodyClass="overflow-y-auto p-2">
        <div className="relative mb-2">
          <Search size={13} className="absolute left-2 top-2.5 text-dim" />
          <input className={inputClass + " w-full pl-7"} placeholder="Filter by name…" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        {assets.map((a) => (
          <button key={a.id} onClick={() => setSel(a.id)} className={cx("cv-auto flex w-full items-center gap-2 rounded-md p-1.5 text-left", asset?.id === a.id ? "bg-raised" : "hover:bg-raised/60")}>
            {a.thumbnail ? <img src={fileUrl(a.thumbnail)} className="h-10 w-16 rounded object-cover" alt="" /> : <div className="h-10 w-16 rounded bg-raised" />}
            <div className="min-w-0 flex-1">
              <div className="truncate text-xs">{a.name}</div>
              <div className="flex gap-1 text-[10px] text-dim">
                <span>{a.role}</span>·<span>{fmtSeconds(ticksToSeconds(a.metadata.duration), 1)}</span>
              </div>
            </div>
            {a.missing && <Badge tone="err">missing</Badge>}
            {!a.rights.confirmed && <Badge tone="warn">rights?</Badge>}
          </button>
        ))}
      </Panel>
      {asset ? <AssetDetail key={asset.id} asset={asset} onProject={setProject} onError={error} /> : <EmptyState title="No media" />}
    </div>
  );
}

function AssetDetail({ asset, onProject, onError }: { asset: MediaAsset; onProject: (p: never) => void; onError: (e: unknown, t?: string) => void }) {
  const va = useQuery({ queryKey: ["video-analysis", asset.id], queryFn: () => api.getVideoAnalysis(asset.id), enabled: asset.kind === "video" });
  const [busy, setBusy] = useState(false);
  const m = asset.metadata;
  const v = m.video;
  const analyze = async () => {
    setBusy(true);
    try {
      await waitForJob(await api.analyzeVideo(asset.id));
      await va.refetch();
    } catch (e) {
      onError(e, "Scene analysis failed");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3 overflow-y-auto">
      <Panel title={asset.name}>
        <div className="grid grid-cols-4 gap-3 text-xs">
          <Info k="Container" v={m.container} />
          <Info k="Duration" v={fmtSeconds(ticksToSeconds(m.duration))} />
          <Info k="Size" v={fmtBytes(m.size_bytes)} />
          <Info k="Kind" v={`${asset.kind} / ${asset.role}`} />
          {v && <Info k="Video" v={`${v.codec} ${v.display_width}×${v.display_height}`} />}
          {v && <Info k="Frame rate" v={`${(v.fps.num / v.fps.den).toFixed(3)}${v.variable_frame_rate ? " (VFR)" : ""}`} />}
          {v && <Info k="Rotation / HDR" v={`${v.rotation}° / ${v.hdr ? "HDR" : "SDR"}`} />}
          {m.audio[0] && <Info k="Audio" v={`${m.audio[0].codec} ${m.audio[0].sample_rate} Hz ${m.audio[0].channels}ch`} />}
        </div>
        <div className="mt-2 truncate text-[11px] text-dim" title={asset.path}>
          {asset.path} · fingerprint {asset.fingerprint.slice(0, 12)}
        </div>
        {m.warnings.map((w) => (
          <div key={w} className="mt-1 text-[11px] text-warn">
            {w}
          </div>
        ))}
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-xs">
            <input type="checkbox" checked={asset.rights.confirmed} onChange={async (e) => onProject((await api.setAssetRights(asset.id, e.target.checked, asset.rights.note)) as never)} />I own or have a license to use this media
          </label>
          {asset.missing && (
            <Button
              size="sm"
              icon={<Link2 size={13} />}
              onClick={async () => {
                const p = await dialogs.pickFile();
                if (!p) return;
                try {
                  onProject((await api.relinkAsset(asset.id, p, false)) as never);
                } catch (e) {
                  if (window.confirm("The selected file has different content. Relink anyway?")) onProject((await api.relinkAsset(asset.id, p, true)) as never);
                  else onError(e);
                }
              }}
            >
              Relink…
            </Button>
          )}
        </div>
      </Panel>
      {asset.kind === "video" && (
        <Panel
          title="Scene analysis"
          actions={
            <Button size="sm" disabled={busy || asset.missing} onClick={analyze} icon={busy ? <Loader2 size={13} className="animate-spin" /> : <ScanSearch size={13} />}>
              {va.data ? "Re-analyze" : "Analyze scenes"}
            </Button>
          }
          bodyClass="p-0"
        >
          {va.data ? (
            <table className="w-full text-xs">
              <thead className="text-left text-dim">
                <tr>
                  <th className="px-3 py-1.5 font-medium">#</th>
                  <th className="font-medium">Time</th>
                  <th className="font-medium">Motion</th>
                  <th className="font-medium">Energy</th>
                  <th className="font-medium">Quality</th>
                  <th className="font-medium">Tags</th>
                </tr>
              </thead>
              <tbody>
                {va.data.scenes.map((s) => (
                  <tr key={s.index} className={cx("cv-auto border-t border-line", !s.usable && "opacity-40")}>
                    <td className="px-3 py-1.5">{s.index}</td>
                    <td className="tabular text-dim">
                      {ticksToSeconds(s.start).toFixed(2)}–{ticksToSeconds(s.end).toFixed(2)}s
                    </td>
                    <td className="tabular">{s.motion.toFixed(2)}</td>
                    <td className="tabular">{s.energy.toFixed(2)}</td>
                    <td className="tabular">{s.quality.toFixed(2)}</td>
                    <td className="text-dim">{s.tags.join(", ")}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyState title="Not analyzed" body="Detects cuts, fades, black frames, motion, camera movement, quality and focus point. Tags are measured signal properties, not semantic recognition." />
          )}
        </Panel>
      )}
    </div>
  );
}

const Info = ({ k, v }: { k: string; v: string }) => (
  <div>
    <div className="text-[11px] text-dim">{k}</div>
    <div className="truncate">{v}</div>
  </div>
);
