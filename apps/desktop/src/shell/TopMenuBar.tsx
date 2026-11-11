import { useQuery } from "@tanstack/react-query";
import { Activity, Boxes, Check, CircleDot, Settings, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import { ProfileSwitcher } from "../components/ProfileSwitcher";
import { Button, Dialog, IconButton, Kbd, cx } from "../components/ui";
import { can, cmd } from "./commands";

type Item =
  | { sep: true }
  | {
      label: string;
      shortcut?: string;
      run: () => unknown;
      enabled?: () => boolean;
      /** Shown as tooltip when disabled. */
      why?: string;
      checked?: () => boolean;
    };

const NOT_IMPL = "Not implemented yet";

function useMenus(openShortcuts: () => void, openAbout: () => void): { name: string; items: Item[] }[] {
  const setRoute = useApp((s) => s.setRoute);
  const toggleLayout = useApp((s) => s.toggleLayout);
  const P = can.project;
  return [
    {
      name: "File",
      items: [
        { label: "New Project…", shortcut: "Ctrl+N", run: cmd.newProject },
        { label: "Open Project…", shortcut: "Ctrl+O", run: cmd.openProject },
        { label: "Save", shortcut: "Ctrl+S", run: cmd.save, enabled: P, why: "No project open" },
        { label: "Close Project", run: cmd.closeProject, enabled: P, why: "No project open" },
        { sep: true },
        { label: "Import Media…", shortcut: "Ctrl+I", run: cmd.importFootage, enabled: P, why: "Open a project first" },
        { label: "Import Music…", run: cmd.importMusic, enabled: P, why: "Open a project first" },
        { label: "Import Folder…", run: cmd.importFolder, enabled: P, why: "Open a project first" },
        { sep: true },
        { label: "Export…", shortcut: "Ctrl+E", run: cmd.export, enabled: P, why: "Open a project first" },
        { sep: true },
        { label: "Projects", run: () => setRoute("projects") },
        { label: "Profiles", run: () => setRoute("profiles") },
        { label: "Settings", run: () => setRoute("settings") },
      ],
    },
    {
      name: "Edit",
      items: [
        { label: `Undo${useApp.getState().undoLabel ? ` ${useApp.getState().undoLabel}` : ""}`, shortcut: "Ctrl+Z", run: cmd.undo, enabled: can.undo, why: "Nothing to undo" },
        { label: `Redo${useApp.getState().redoLabel ? ` ${useApp.getState().redoLabel}` : ""}`, shortcut: "Ctrl+Y", run: cmd.redo, enabled: can.redo, why: "Nothing to redo" },
        { sep: true },
        { label: "Cut", shortcut: "Ctrl+X", run: () => undefined, enabled: () => false, why: NOT_IMPL },
        { label: "Copy", shortcut: "Ctrl+C", run: () => undefined, enabled: () => false, why: NOT_IMPL },
        { label: "Paste", shortcut: "Ctrl+V", run: () => undefined, enabled: () => false, why: NOT_IMPL },
        { sep: true },
        { label: "Delete", shortcut: "Del", run: () => cmd.deleteSelected(false), enabled: can.unlockedSelection, why: "Select an unlocked clip" },
        { label: "Ripple Delete", shortcut: "Shift+Del", run: () => cmd.deleteSelected(true), enabled: can.unlockedSelection, why: "Select an unlocked clip" },
        { label: "Clear Range Selection", shortcut: "Esc", run: cmd.clearRange, enabled: can.range, why: "No range selected" },
      ],
    },
    {
      name: "Clip",
      items: [
        { label: "Split at Playhead", shortcut: "S", run: () => cmd.split(), enabled: can.split, why: "No unlocked clip under the playhead" },
        { label: "Lock / Unlock Clip", shortcut: "L", run: cmd.toggleLock, enabled: can.selection, why: "Select a clip" },
        { label: "Lock Clips in Range", run: cmd.lockRange, enabled: can.range, why: "Shift+drag on the ruler to select a range" },
        { label: "Replace Scene", run: cmd.replaceScene, enabled: () => can.unlockedSelection() && can.edit(), why: "Select an unlocked clip of a generated edit" },
        { sep: true },
        { label: "Reset Framing & Keyframes", run: cmd.resetFraming, enabled: can.unlockedSelection, why: "Select a clip" },
        { label: "Remove All Effects", run: cmd.clearEffects, enabled: can.selection, why: "Select a clip" },
        { sep: true },
        { label: "Speed Curve…", run: () => undefined, enabled: () => false, why: "Speed curves are not implemented (constant speed is in the Inspector → Speed tab)" },
      ],
    },
    {
      name: "Sequence",
      items: [
        { label: "Generate Edit", run: cmd.generate, enabled: () => can.project() && can.music(), why: "Import music first" },
        { label: "Regenerate Unlocked", run: cmd.regenUnlocked, enabled: can.edit, why: "Generate an edit first" },
        { label: "Regenerate Range", run: cmd.regenRange, enabled: () => can.edit() && can.range(), why: "Select a range on the ruler" },
        { label: "Reapply Effects", run: cmd.reapplyEffects, enabled: can.edit, why: "Generate an edit first" },
        { sep: true },
        { label: "Add Marker", shortcut: "M", run: cmd.addMarker, enabled: P, why: "Open a project first" },
        { label: "Add Video Track", run: () => cmd.addTrack("video"), enabled: P, why: "Open a project first" },
        { label: "Add Audio Track", run: () => cmd.addTrack("audio"), enabled: P, why: "Open a project first" },
        { sep: true },
        { label: "Render Preview for Playback", run: cmd.renderPreview, enabled: can.edit, why: "Nothing to preview" },
      ],
    },
    {
      name: "View",
      items: [
        { label: "Tool Panel", run: () => toggleLayout("left"), checked: () => useApp.getState().layout.left },
        { label: "Inspector", run: () => toggleLayout("right"), checked: () => useApp.getState().layout.right },
        { label: "Timeline", run: () => toggleLayout("bottom"), checked: () => useApp.getState().layout.bottom },
        { label: "Reset Panel Sizes", run: resetLayout },
        { sep: true },
        { label: "Zoom In Timeline", shortcut: "+", run: cmd.zoomIn, enabled: P },
        { label: "Zoom Out Timeline", shortcut: "-", run: cmd.zoomOut, enabled: P },
        { label: "Fit Timeline", shortcut: "Shift+Z", run: cmd.zoomFit, enabled: P },
        { sep: true },
        { label: "Dashboard", run: () => setRoute("home") },
        { label: "Editor", run: () => setRoute("editor"), enabled: P, why: "Open a project first" },
        { label: "Media Library", run: () => setRoute("library"), enabled: P, why: "Open a project first" },
        { label: "Audio Intelligence", run: () => setRoute("audio"), enabled: P, why: "Open a project first" },
        { label: "Effects & Presets", run: () => setRoute("presets") },
        { label: "Clip Discovery", run: () => setRoute("discover") },
        { label: "Export History", run: () => setRoute("export"), enabled: P, why: "Open a project first" },
      ],
    },
    {
      name: "Help",
      items: [
        { label: "Keyboard Shortcuts", shortcut: "?", run: openShortcuts },
        { label: "About AI Auto Edit Studio", run: openAbout },
      ],
    },
  ];
}

function resetLayout() {
  try {
    for (const k of Object.keys(localStorage)) if (k.startsWith("panel:")) localStorage.removeItem(k);
  } catch {
    /* ignore */
  }
  useApp.getState().toggleLayout("left", true);
  useApp.getState().toggleLayout("right", true);
  useApp.getState().toggleLayout("bottom", true);
  window.location.reload();
}

function MenuDropdown({ items, close }: { items: Item[]; close: () => void }) {
  // Re-render while open so enabled states track the editor.
  useApp((s) => s.project);
  useApp((s) => s.selectedClipId);
  useApp((s) => s.range);
  useApp((s) => s.canUndo);
  useApp((s) => s.canRedo);
  useApp((s) => s.layout);
  return (
    <div role="menu" className="absolute left-0 top-full z-[60] mt-0.5 min-w-60 rounded-md border border-line bg-surface p-1 shadow-2xl">
      {items.map((it, i) =>
        "sep" in it ? (
          <div key={i} className="my-1 h-px bg-line" />
        ) : (
          (() => {
            const enabled = it.enabled ? it.enabled() : true;
            const checked = it.checked?.();
            return (
              <button
                key={i}
                role={checked === undefined ? "menuitem" : "menuitemcheckbox"}
                aria-checked={checked}
                disabled={!enabled}
                title={!enabled ? it.why : undefined}
                onClick={() => {
                  close();
                  void it.run();
                }}
                className="flex h-7 w-full items-center gap-2 rounded px-2 text-left text-xs text-fg hover:bg-accent hover:text-white disabled:text-dim disabled:hover:bg-transparent"
              >
                <span className="w-3.5 shrink-0">{checked && <Check size={12} />}</span>
                <span className="flex-1 truncate">{it.label}</span>
                {it.shortcut && <span className="ml-6 text-[10px] opacity-60">{it.shortcut}</span>}
              </button>
            );
          })()
        ),
      )}
    </div>
  );
}

function SaveStatus() {
  const project = useApp((s) => s.project);
  const dirty = useApp((s) => s.dirty);
  const lastSavedAt = useApp((s) => s.lastSavedAt);
  if (!project) return null;
  return (
    <span className="flex items-center gap-1.5 text-[11px] text-dim" title={dirty ? "Unsaved changes. Autosave keeps a recovery copy every 30 s; Ctrl+S saves the project file." : "All changes saved"}>
      <CircleDot size={9} className={dirty ? "text-warn" : "text-ok"} />
      {dirty ? "Unsaved" : lastSavedAt ? "Saved" : "Up to date"}
    </span>
  );
}

export function TopMenuBar() {
  const project = useApp((s) => s.project);
  const route = useApp((s) => s.route);
  const setRoute = useApp((s) => s.setRoute);
  const jobs = useApp((s) => s.jobs);
  const jobsOpen = useApp((s) => s.jobsOpen);
  const setJobsOpen = useApp((s) => s.setJobsOpen);
  const [open, setOpen] = useState<string | null>(null);
  const [shortcuts, setShortcuts] = useState(false);
  const [about, setAbout] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  const menus = useMenus(
    () => setShortcuts(true),
    () => setAbout(true),
  );
  const active = Object.values(jobs).filter((j) => j.status === "running" || j.status === "queued").length;

  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => !barRef.current?.contains(e.target as Node) && setOpen(null);
    const k = (e: KeyboardEvent) => e.key === "Escape" && setOpen(null);
    window.addEventListener("mousedown", h);
    window.addEventListener("keydown", k);
    return () => {
      window.removeEventListener("mousedown", h);
      window.removeEventListener("keydown", k);
    };
  }, [open]);

  // Global shortcuts (timeline-specific keys live in the timeline).
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      const key = e.key.toLowerCase();
      const st = useApp.getState();
      if (e.ctrlKey && !e.altKey) {
        if (key === "s") {
          e.preventDefault();
          void cmd.save();
        } else if (key === "z" && !e.shiftKey) {
          e.preventDefault();
          void cmd.undo();
        } else if (key === "y" || (key === "z" && e.shiftKey)) {
          e.preventDefault();
          void cmd.redo();
        } else if (key === "e") {
          e.preventDefault();
          cmd.export();
        } else if (key === "n") {
          e.preventDefault();
          cmd.newProject();
        } else if (key === "o") {
          e.preventDefault();
          void cmd.openProject();
        } else if (key === "i" && st.project) {
          e.preventDefault();
          void cmd.importFootage();
        }
        return;
      }
      if (e.altKey) return;
      if (e.key === "?") setShortcuts(true);
      if (st.route !== "editor" || !st.project) return;
      if (key === "m") cmd.addMarker();
      else if (key === "v") st.setTlTool("select");
      else if (key === "c") st.setTlTool("razor");
      else if (key === "z" && e.shiftKey) cmd.zoomFit();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  return (
    <header className="flex h-[42px] shrink-0 items-center border-b border-line bg-surface pl-2 pr-2">
      <div ref={barRef} className="flex h-full items-center">
        <button className="mr-1 flex h-7 w-7 items-center justify-center rounded-md text-accent hover:bg-raised" title="Dashboard" onClick={() => setRoute("home")}>
          <Boxes size={17} />
        </button>
        {menus.map((m) => (
          <div key={m.name} className="relative">
            <button
              className={cx("h-7 rounded px-2.5 text-xs", open === m.name ? "bg-raised text-fg" : "text-muted hover:bg-raised hover:text-fg")}
              aria-haspopup="menu"
              aria-expanded={open === m.name}
              onMouseDown={(e) => {
                e.preventDefault();
                setOpen(open === m.name ? null : m.name);
              }}
              onMouseEnter={() => open && setOpen(m.name)}
            >
              {m.name}
            </button>
            {open === m.name && <MenuDropdown items={m.items} close={() => setOpen(null)} />}
          </div>
        ))}
      </div>

      <div className="flex min-w-0 flex-1 items-center justify-center gap-3 px-4">
        {project ? (
          <>
            <button className="max-w-72 truncate text-xs font-medium hover:text-accent" title={`${project.name} — open editor`} onClick={() => setRoute("editor")}>
              {project.name}
            </button>
            <span className="hidden text-[11px] text-dim lg:inline">
              {project.settings.width}×{project.settings.height} · {Number((project.settings.fps.num / project.settings.fps.den).toFixed(2))} fps
            </span>
            <SaveStatus />
          </>
        ) : (
          <span className="text-xs text-dim">{route === "home" ? "AI Auto Edit Studio" : "No project open"}</span>
        )}
      </div>

      <div className="flex items-center gap-1">
        <button
          className={cx("flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px]", jobsOpen ? "bg-raised text-fg" : "text-muted hover:bg-raised hover:text-fg")}
          title="Background jobs"
          onClick={() => setJobsOpen(!jobsOpen)}
        >
          <Activity size={14} className={active ? "text-accent" : undefined} />
          {active ? `${active} running` : "Jobs"}
        </button>
        <Button size="sm" variant="primary" className="ml-1 h-7 px-3" icon={<Upload size={13} />} disabled={!project} onClick={cmd.export} title="Export (Ctrl+E)">
          Export
        </Button>
        <IconButton title="Settings" active={route === "settings"} onClick={() => setRoute("settings")}>
          <Settings size={15} />
        </IconButton>
        <div className="mx-1 h-5 w-px bg-line" />
        <ProfileSwitcher compact />
      </div>

      <ShortcutsDialog open={shortcuts} onClose={() => setShortcuts(false)} />
      <AboutDialog open={about} onClose={() => setAbout(false)} />
    </header>
  );
}

const SHORTCUTS: [string, string][] = [
  ["Space", "Play / pause preview"],
  ["← / →", "Previous / next frame (Shift: 10 frames)"],
  ["Home / End", "Go to start / end"],
  ["S", "Split at playhead"],
  ["V / C", "Select tool / razor tool"],
  ["L", "Lock / unlock selected clip"],
  ["M", "Add marker at playhead"],
  ["Del / Shift+Del", "Delete / ripple delete"],
  ["Shift+drag ruler", "Select a range (regenerate / lock range)"],
  ["+ / - / Shift+Z", "Zoom timeline in / out / fit"],
  ["Esc", "Clear range selection"],
  ["Ctrl+Z / Ctrl+Y", "Undo / redo"],
  ["Ctrl+S", "Save project"],
  ["Ctrl+E", "Export"],
  ["Ctrl+N / Ctrl+O / Ctrl+I", "New project / open / import media"],
];

function ShortcutsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onClose={onClose} title="Keyboard shortcuts" width={460}>
      <table className="w-full text-xs">
        <tbody>
          {SHORTCUTS.map(([k, v]) => (
            <tr key={k} className="border-b border-line last:border-0">
              <td className="py-1.5 pr-4">
                {k.split(" / ").map((p, i) => (
                  <span key={p}>
                    {i > 0 && <span className="mx-1 text-dim">/</span>}
                    <Kbd>{p}</Kbd>
                  </span>
                ))}
              </td>
              <td className="py-1.5 text-muted">{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Dialog>
  );
}

function AboutDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo, enabled: open });
  return (
    <Dialog open={open} onClose={onClose} title="About" width={440}>
      <div className="flex flex-col gap-2 text-xs">
        <div className="text-sm font-semibold">AI Auto Edit Studio {info.data?.version}</div>
        <div className="text-muted">Local music-synchronized short-form editor. Rust core, Tauri 2, React.</div>
        <div className="mt-2 grid grid-cols-[100px_1fr] gap-y-1 text-[11px]">
          <span className="text-dim">FFmpeg</span>
          <span className="truncate">{info.data?.ffmpeg ? `${info.data.ffmpeg.source} · ${info.data.ffmpeg.ffmpeg}` : info.data?.ffmpeg_error ?? "not found"}</span>
          <span className="text-dim">CPU threads</span>
          <span>{info.data?.cpu_threads}</span>
          <span className="text-dim">Data folder</span>
          <span className="truncate" title={info.data?.data_dir}>
            {info.data?.data_dir}
          </span>
        </div>
      </div>
    </Dialog>
  );
}
