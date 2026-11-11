import { FolderOpen, Play, Upload } from "lucide-react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { fmtDate, fmtSeconds, ticksToSeconds } from "../lib/time";
import { Badge, Button, EmptyState, IconButton, Panel } from "../components/ui";
import { FfmpegBanner } from "../components/FfmpegBanner";

/** Export history of the open project; new exports go through the export dialog. */
export default function ExportCenter() {
  const project = useApp((s) => s.project);
  const setExportOpen = useApp((s) => s.setExportOpen);
  const error = useApp((s) => s.error);
  if (!project) return <EmptyState title="No project open" />;
  const open = (p: string, reveal: boolean) => api.openPath(p, reveal).catch((e) => error(e, "Could not open"));
  return (
    <div className="h-full overflow-y-auto p-5">
      <div className="mx-auto flex max-w-5xl flex-col gap-3">
        <FfmpegBanner />
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-sm font-semibold">Export history</h1>
            <p className="text-[11px] text-dim">Every export of “{project.name}”, validated with ffprobe after rendering.</p>
          </div>
          <Button variant="primary" icon={<Upload size={13} />} onClick={() => setExportOpen(true)}>
            New export…
          </Button>
        </div>
        <Panel bodyClass="overflow-auto p-0">
          {project.renders.length ? (
            <table className="w-full text-xs">
              <thead className="text-left text-[11px] text-dim">
                <tr>
                  <th className="px-3 py-2 font-medium">File</th>
                  <th className="px-2 font-medium">Size</th>
                  <th className="px-2 font-medium">Length</th>
                  <th className="px-2 font-medium">Encoder</th>
                  <th className="px-2 font-medium">Date</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {[...project.renders].reverse().map((r) => (
                  <tr key={r.id} className="border-t border-line hover:bg-raised/50">
                    <td className="max-w-md truncate px-3 py-1.5 text-muted" title={r.output}>
                      {r.output}
                    </td>
                    <td className="px-2 text-dim">
                      {r.width}×{r.height}
                    </td>
                    <td className="px-2 text-dim">{fmtSeconds(ticksToSeconds(r.duration), 1)}</td>
                    <td className="px-2">
                      <Badge tone={r.warnings.length ? "warn" : "ok"}>{r.encoder}</Badge>
                    </td>
                    <td className="px-2 text-dim">{fmtDate(r.created_at)}</td>
                    <td className="px-2 text-right">
                      <IconButton title="Show in folder" onClick={() => open(r.output, true)}>
                        <FolderOpen size={13} />
                      </IconButton>
                      <IconButton title="Open file" onClick={() => open(r.output, false)}>
                        <Play size={13} />
                      </IconButton>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyState title="Nothing exported yet" action={<Button onClick={() => setExportOpen(true)}>Export…</Button>} />
          )}
        </Panel>
      </div>
    </div>
  );
}
