import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Library, Music, Plus, Search, Share2 } from "lucide-react";
import { useState } from "react";
import { api } from "../services/api";
import { useApp, waitForJob } from "../stores/app";
import { fmtSeconds } from "../lib/time";
import { Badge, EmptyState, IconButton, inputClass, PanelHeader, Progress } from "../components/ui";

/** The active profile's media library (plus media shared by other profiles). */
export function LibraryPanel() {
  const project = useApp((s) => s.project);
  const profile = useApp((s) => s.profile);
  const toast = useApp((s) => s.toast);
  const error = useApp((s) => s.error);
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const lib = useQuery({ queryKey: ["profile-assets", profile?.id], queryFn: api.profileAssets });
  const inProject = new Set(project?.assets.map((a) => a.path.toLowerCase()) ?? []);
  const list = (lib.data ?? []).filter((a) => a.path.toLowerCase().includes(q.toLowerCase()));
  const name = (p: string) => p.split(/[\\/]/).pop() ?? p;

  const add = async (path: string, kind: string) => {
    try {
      const role = kind === "audio" ? "music" : "footage";
      if (role === "music" && project?.assets.some((a) => a.role === "music") && !window.confirm("Replace the project's music with this track?")) return;
      await waitForJob(await api.importMedia([path], role));
      toast({ kind: "success", title: `Added ${name(path)}` });
    } catch (e) {
      error(e, "Import failed");
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader title="Library" actions={profile && <Badge>{profile.name}</Badge>} />
      <div className="border-b border-line p-2">
        <div className="relative">
          <Search size={12} className="absolute left-2 top-2.5 text-dim" />
          <input className={inputClass + " h-7 w-full pl-6"} placeholder="Search this profile's media…" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-1">
        {lib.isLoading && <Progress value={null} className="m-2" />}
        {!lib.isLoading && list.length === 0 && <EmptyState icon={<Library size={22} />} title="Library is empty" body="Media imported into this profile's projects appears here and can be reused across projects." />}
        {list.map((a) => {
          const used = inProject.has(a.path.toLowerCase());
          return (
            <div key={a.fingerprint} className="cv-auto flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-raised" title={a.path}>
              <span className="text-dim">{a.kind === "audio" ? <Music size={13} /> : <span className="text-[10px]">{a.kind.slice(0, 3)}</span>}</span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-[11px]">{name(a.path)}</div>
                <div className="text-[10px] text-dim">
                  {fmtSeconds(a.duration_s, 1)}
                  {a.owner_profile !== profile?.id && " · shared"}
                  {used && " · in project"}
                </div>
              </div>
              {a.owner_profile === profile?.id && (
                <IconButton
                  title={a.shared ? "Shared with all profiles (click to make private)" : "Private to this profile (click to share)"}
                  active={a.shared}
                  onClick={async () => {
                    try {
                      await api.setAssetShared(a.fingerprint, !a.shared);
                      await qc.invalidateQueries({ queryKey: ["profile-assets"] });
                    } catch (e) {
                      error(e);
                    }
                  }}
                >
                  <Share2 size={12} />
                </IconButton>
              )}
              <IconButton title={used ? "Already in this project" : "Add to project"} disabled={used || !project} onClick={() => add(a.path, a.kind)}>
                <Plus size={13} />
              </IconButton>
            </div>
          );
        })}
      </div>
    </div>
  );
}
