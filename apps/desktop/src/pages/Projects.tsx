import { useQuery } from "@tanstack/react-query";
import { FolderOpen } from "lucide-react";
import { api, dialogs } from "../services/api";
import { useApp } from "../stores/app";
import { fmtDate } from "../lib/time";
import { Button, EmptyState, Panel } from "../components/ui";

/** Open a project (file dialog if no path), offering autosave recovery and reporting missing media. */
export async function openProjectFlow(path?: string) {
  const st = useApp.getState();
  const p = path ?? (await dialogs.pickProject());
  if (!p) return;
  try {
    const r = await api.openProject(p);
    let project = r.project;
    if (r.autosave_available && window.confirm("A newer autosave exists (the app may have closed unexpectedly). Restore it?")) {
      project = await api.recoverAutosave(p);
    }
    st.setProject(project);
    if (r.missing_assets.length) {
      st.toast({ kind: "error", title: `${r.missing_assets.length} media file(s) are missing. Relink them in the Media Library.` });
      st.setRoute("library");
    } else {
      st.setRoute(project.timeline.tracks.some((t) => t.clips.length) ? "editor" : "auto");
    }
  } catch (e) {
    st.error(e, "Could not open project");
  }
}

export default function Projects() {
  const recent = useQuery({ queryKey: ["recent-projects"], queryFn: api.recentProjects });
  const { setNewProjectOpen } = useApp();
  return (
    <div className="h-full overflow-y-auto p-5">
      <div className="mx-auto max-w-4xl">
        <div className="mb-3 flex gap-2">
          <Button variant="primary" onClick={() => setNewProjectOpen(true)}>
            New project
          </Button>
          <Button icon={<FolderOpen size={14} />} onClick={() => openProjectFlow()}>
            Open…
          </Button>
        </div>
        <Panel title="All recent projects" bodyClass="overflow-auto">
          {recent.data?.length ? (
            <table className="w-full text-xs">
              <thead className="text-left text-dim">
                <tr>
                  <th className="px-3 py-2 font-medium">Name</th>
                  <th className="px-3 font-medium">Location</th>
                  <th className="px-3 font-medium">Modified</th>
                </tr>
              </thead>
              <tbody>
                {recent.data.map((p) => (
                  <tr key={p.id} className="cv-auto cursor-pointer border-t border-line hover:bg-raised" onClick={() => openProjectFlow(p.path)}>
                    <td className="px-3 py-2">{p.name}</td>
                    <td className="max-w-md truncate px-3 text-dim">{p.path}</td>
                    <td className="px-3 text-dim">{fmtDate(p.modified_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyState title="No projects" />
          )}
        </Panel>
      </div>
    </div>
  );
}
