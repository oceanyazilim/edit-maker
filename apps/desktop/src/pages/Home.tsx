import { useQuery } from "@tanstack/react-query";
import { Clapperboard, FileVideo, FolderOpen, Plus, Wand2 } from "lucide-react";
import { api, dialogs } from "../services/api";
import { useApp } from "../stores/app";
import { fmtDate, fmtSeconds } from "../lib/time";
import { Badge, Button, EmptyState, Panel, Progress } from "../components/ui";
import { FfmpegBanner } from "../components/FfmpegBanner";
import { openProjectFlow } from "./Projects";

export default function Home() {
  const { setNewProjectOpen, jobs } = useApp();
  const recent = useQuery({ queryKey: ["recent-projects"], queryFn: api.recentProjects });
  const renders = useQuery({ queryKey: ["recent-renders"], queryFn: api.recentRenders });
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const active = Object.values(jobs).filter((j) => j.status === "running" || j.status === "queued");

  return (
    <div className="h-full overflow-y-auto p-5">
      <div className="mx-auto flex max-w-6xl flex-col gap-4">
        <FfmpegBanner />
        <div className="grid grid-cols-3 gap-3">
          <button onClick={() => setNewProjectOpen(true)} className="group flex items-center gap-3 rounded-lg border border-line bg-surface p-4 text-left hover:border-line-strong">
            <div className="flex h-10 w-10 items-center justify-center rounded-md bg-accent/15 text-accent">
              <Wand2 size={18} />
            </div>
            <div>
              <div className="text-sm font-medium">New Auto Edit</div>
              <div className="text-xs text-muted">Music + footage → beat-synchronized video</div>
            </div>
          </button>
          <button onClick={() => setNewProjectOpen(true)} className="flex items-center gap-3 rounded-lg border border-line bg-surface p-4 text-left hover:border-line-strong">
            <div className="flex h-10 w-10 items-center justify-center rounded-md bg-raised text-fg">
              <Clapperboard size={18} />
            </div>
            <div>
              <div className="text-sm font-medium">New Manual Project</div>
              <div className="text-xs text-muted">Start with an empty timeline</div>
            </div>
          </button>
          <button onClick={() => openProjectFlow()} className="flex items-center gap-3 rounded-lg border border-line bg-surface p-4 text-left hover:border-line-strong">
            <div className="flex h-10 w-10 items-center justify-center rounded-md bg-raised text-fg">
              <FolderOpen size={18} />
            </div>
            <div>
              <div className="text-sm font-medium">Open Project</div>
              <div className="text-xs text-muted">.aesproj file</div>
            </div>
          </button>
        </div>

        <div className="grid grid-cols-3 gap-4">
          <Panel title="Recent projects" className="col-span-2 h-80" bodyClass="overflow-auto">
            {recent.data?.length ? (
              <ul>
                {recent.data.map((p) => (
                  <li key={p.id}>
                    <button className="flex w-full items-center justify-between border-b border-line px-3 py-2 text-left hover:bg-raised" onClick={() => openProjectFlow(p.path)}>
                      <span className="truncate">{p.name}</span>
                      <span className="text-[11px] text-dim">{fmtDate(p.modified_at)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <EmptyState icon={<Plus size={22} />} title="No projects yet" body="Create an auto edit to get started." action={<Button onClick={() => setNewProjectOpen(true)}>New project</Button>} />
            )}
          </Panel>
          <Panel title="Active jobs" className="h-80">
            {active.length ? (
              <div className="flex flex-col gap-3">
                {active.map((j) => (
                  <div key={j.id}>
                    <div className="mb-1 flex justify-between text-xs">
                      <span className="truncate">{j.label}</span>
                      <span className="text-dim">{j.progress != null ? `${Math.round(j.progress * 100)}%` : ""}</span>
                    </div>
                    <Progress value={j.progress} />
                    <div className="mt-1 text-[11px] text-dim">{j.stage}</div>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState title="Nothing running" body="Imports, analyses and renders appear here." />
            )}
          </Panel>
          <Panel title="Recent exports" className="col-span-2 h-64" bodyClass="overflow-auto">
            {renders.data?.length ? (
              <table className="w-full text-xs">
                <tbody>
                  {renders.data.map((r) => (
                    <tr key={r.id} className="border-b border-line">
                      <td className="px-3 py-1.5">
                        <FileVideo size={13} className="mr-2 inline text-dim" />
                        <span className="text-muted">{r.output}</span>
                      </td>
                      <td className="px-2 text-dim">{fmtSeconds(r.duration_s, 1)}</td>
                      <td className="px-2 text-dim">{r.encoder}</td>
                      <td className="px-2">{r.ok ? <Badge tone="ok">ok</Badge> : <Badge tone="err">failed</Badge>}</td>
                      <td className="px-3 text-right text-dim">{fmtDate(r.created_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <EmptyState title="No exports yet" />
            )}
          </Panel>
          <Panel title="System" className="h-64">
            <div className="flex flex-col gap-2 text-xs">
              <div className="flex justify-between">
                <span className="text-muted">FFmpeg</span>
                {info.data?.ffmpeg ? <Badge tone="ok">{info.data.ffmpeg.source}</Badge> : <Badge tone="warn">missing</Badge>}
              </div>
              <div className="flex justify-between">
                <span className="text-muted">CPU threads</span>
                <span>{info.data?.cpu_threads ?? "—"}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted">Presets</span>
                <span>{presets.data?.length ?? "—"}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted">Trend sync</span>
                <span className="text-dim">no provider connected</span>
              </div>
              <div className="mt-2 truncate text-[11px] text-dim" title={info.data?.projects_dir}>
                Projects: {info.data?.projects_dir}
              </div>
              <Button size="sm" variant="ghost" className="mt-auto self-start" onClick={async () => { const p = await dialogs.pickProject(); if (p) void openProjectFlow(p); }}>
                Browse…
              </Button>
            </div>
          </Panel>
        </div>
      </div>
    </div>
  );
}
