import { AudioWaveform, Clapperboard, Compass, FolderOpen, Home, Layers, Library, Megaphone, ScanSearch, Sparkles, TrendingUp, Upload, Users, Wand2 } from "lucide-react";
import type { ReactNode } from "react";
import { useApp, type Route } from "../stores/app";
import { cx } from "./ui";

const items: { route: Route; label: string; icon: ReactNode; planned?: boolean; needsProject?: boolean; group?: string }[] = [
  { route: "home", label: "Dashboard", icon: <Home size={15} /> },
  { route: "projects", label: "Projects", icon: <FolderOpen size={15} /> },
  { route: "auto", label: "Auto Edit", icon: <Wand2 size={15} />, needsProject: true, group: "Project" },
  { route: "editor", label: "Editor", icon: <Clapperboard size={15} />, needsProject: true },
  { route: "audio", label: "Audio Intelligence", icon: <AudioWaveform size={15} />, needsProject: true },
  { route: "library", label: "Media Library", icon: <Library size={15} />, needsProject: true },
  { route: "export", label: "Export History", icon: <Upload size={15} />, needsProject: true },
  { route: "discover", label: "Clip Discovery", icon: <ScanSearch size={15} />, group: "Workspace" },
  { route: "niches", label: "Niche Explorer", icon: <Compass size={15} /> },
  { route: "presets", label: "Effects & Presets", icon: <Sparkles size={15} /> },
  { route: "profiles", label: "Profiles", icon: <Users size={15} /> },
  { route: "trends", label: "Trend Intelligence", icon: <TrendingUp size={15} />, planned: true, group: "Planned" },
  { route: "batch", label: "Batch Studio", icon: <Layers size={15} />, planned: true },
  { route: "campaigns", label: "Campaign Studio", icon: <Megaphone size={15} />, planned: true },
];

/** Navigation for the dashboard / browser pages (the editor uses the tool rail instead). */
export function Sidebar() {
  const route = useApp((s) => s.route);
  const setRoute = useApp((s) => s.setRoute);
  const project = useApp((s) => s.project);
  return (
    <nav aria-label="Pages" className="flex w-52 shrink-0 flex-col overflow-y-auto border-r border-line bg-surface py-2">
      {items.map((it) => {
        const disabled = it.needsProject && !project;
        return (
          <div key={it.route}>
            {it.group && <div className="mx-4 mb-1 mt-4 text-[10px] font-semibold uppercase tracking-widest text-dim">{it.group}</div>}
            <button
              disabled={disabled}
              onClick={() => setRoute(it.route)}
              title={disabled ? "Open or create a project first" : it.planned ? `${it.label} — planned, not implemented` : it.label}
              className={cx(
                "relative mx-2 flex w-[calc(100%-1rem)] items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors disabled:opacity-35",
                route === it.route ? "bg-raised text-fg" : "text-muted hover:bg-raised/60 hover:text-fg",
                it.planned && "text-dim",
              )}
            >
              {route === it.route && <span className="absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-full bg-accent" />}
              <span className={route === it.route ? "text-accent" : undefined}>{it.icon}</span>
              <span className="truncate">{it.label}</span>
            </button>
          </div>
        );
      })}
    </nav>
  );
}
