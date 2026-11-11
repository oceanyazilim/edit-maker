import { Activity, X } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { Badge, EmptyState, IconButton, Progress } from "./ui";

/** Background jobs popover, opened from the menu bar. */
export function JobsTray() {
  const jobs = useApp((s) => s.jobs);
  const open = useApp((s) => s.jobsOpen);
  const setOpen = useApp((s) => s.setJobsOpen);
  const profileId = useApp((s) => s.profile?.id);
  const ref = useRef<HTMLDivElement>(null);
  const list = useMemo(() => Object.values(jobs).sort((a, b) => b.created_at - a.created_at || b.id.localeCompare(a.id)), [jobs]);

  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!ref.current?.contains(t) && !t.closest("[title='Background jobs']")) setOpen(false);
    };
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open, setOpen]);

  if (!open) return null;
  return (
    <div ref={ref} className="fixed right-3 top-[46px] z-50 flex max-h-[70vh] w-96 flex-col rounded-lg border border-line bg-surface shadow-2xl">
      <header className="flex h-9 shrink-0 items-center justify-between border-b border-line px-3">
        <span className="flex items-center gap-2 text-xs font-medium">
          <Activity size={13} /> Jobs
        </span>
        <IconButton title="Close" onClick={() => setOpen(false)}>
          <X size={13} />
        </IconButton>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {list.length === 0 && <EmptyState title="No jobs yet" body="Imports, analyses, generation and exports run here in the background." />}
        {list.slice(0, 40).map((j) => (
          <div key={j.id} className="border-b border-line px-3 py-2 last:border-0">
            <div className="flex items-center justify-between gap-2">
              <span className="truncate text-xs">
                {j.label}
                {j.owner && j.owner !== profileId && <span className="ml-1 text-[10px] text-dim">(other profile)</span>}
              </span>
              <div className="flex items-center gap-1">
                <Badge tone={j.status === "completed" ? "ok" : j.status === "failed" ? "err" : j.status === "running" ? "accent" : "neutral"}>{j.status}</Badge>
                {(j.status === "running" || j.status === "queued") && (
                  <IconButton title="Cancel job" onClick={() => api.cancelJob(j.id)}>
                    <X size={13} />
                  </IconButton>
                )}
              </div>
            </div>
            <div className="mt-1 truncate text-[11px] text-dim">
              {j.stage}
              {j.elapsed_ms != null && ` · ${(j.elapsed_ms / 1000).toFixed(1)}s`}
            </div>
            {j.status === "running" && <Progress className="mt-1" value={j.progress} />}
            {j.error && (
              <details className="mt-1 text-[11px] text-err">
                <summary className="cursor-pointer">{j.error.message}</summary>
                {j.error.details && <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap text-dim">{j.error.details}</pre>}
              </details>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
