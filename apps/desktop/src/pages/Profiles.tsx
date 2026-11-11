import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, ArchiveRestore, Copy, Download, Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import type { Profile } from "../types/studio";
import { fmtDate } from "../lib/time";
import { Badge, Button, IconButton, inputClass, Panel } from "../components/ui";
import { Avatar, switchProfileFlow } from "../components/ProfileSwitcher";

export default function Profiles() {
  const { profile: active, setProfileDialog, toast, error } = useApp();
  const qc = useQueryClient();
  const [q, setQ] = useState("");
  const [showArchived, setShowArchived] = useState(false);
  const list = useQuery({ queryKey: ["profiles", "manage", q, showArchived], queryFn: () => api.listProfiles(q, showArchived) });
  const refresh = () => qc.invalidateQueries({ queryKey: ["profiles"] });

  const act = async (f: () => Promise<unknown>, ok: string) => {
    try {
      await f();
      toast({ kind: "success", title: ok });
      await refresh();
    } catch (e) {
      error(e);
    }
  };

  const remove = async (p: Profile) => {
    const s = await api.profileStats(p.id);
    const msg =
      `Permanently delete "${p.name}"?\n\n` +
      `• ${s.projects} project entries and ${s.renders} export records are removed from the library.\n` +
      `• ${s.assets} media references are removed. Media still used by other profiles stays available.\n` +
      `• Project files and media files on disk are NOT deleted.\n\nThis cannot be undone.`;
    if (!window.confirm(msg)) return;
    await act(async () => {
      const r = await api.deleteProfile(p.id);
      toast({ kind: "info", title: `Removed ${r.project_entries_removed} projects, ${r.asset_references_removed} media references` });
    }, "Profile deleted");
  };

  const exportConfig = async (p: Profile) => {
    try {
      const json = await api.exportProfileConfig(p.id);
      const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = `${p.name.replace(/[^\w-]+/g, "_")}.aes-profile.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      error(e);
    }
  };

  return (
    <div className="h-full overflow-y-auto p-5">
      <div className="mx-auto max-w-5xl">
        <div className="mb-3 flex items-center gap-2">
          <Button variant="primary" icon={<Plus size={14} />} onClick={() => setProfileDialog({ open: true, edit: null })}>New profile</Button>
          <input className={inputClass + " w-64"} placeholder="Search profiles…" value={q} onChange={(e) => setQ(e.target.value)} />
          <label className="ml-auto flex items-center gap-2 text-xs text-muted">
            <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} /> Show archived
          </label>
        </div>
        <Panel title="Profiles" bodyClass="p-0">
          <table className="w-full text-xs">
            <tbody>
              {(list.data ?? []).map((p) => (
                <tr key={p.id} className="border-b border-line">
                  <td className="px-3 py-2">
                    <div className="flex items-center gap-2">
                      <Avatar p={p} />
                      <div>
                        <div className="font-medium">{p.name} {p.id === active?.id && <Badge tone="accent">active</Badge>} {p.archived && <Badge>archived</Badge>}</div>
                        <div className="text-[11px] text-dim">{p.description || p.primary_niche || "—"}</div>
                      </div>
                    </div>
                  </td>
                  <td className="px-2 text-dim">{p.defaults.preset_id}</td>
                  <td className="px-2 text-dim">{p.defaults.aspect} · {p.defaults.duration_secs}s</td>
                  <td className="px-2 text-dim">{fmtDate(p.updated_at)}</td>
                  <td className="px-3">
                    <div className="flex justify-end gap-0.5">
                      {!p.archived && p.id !== active?.id && <Button size="sm" onClick={() => switchProfileFlow(p, qc)}>Switch</Button>}
                      <IconButton title="Edit" onClick={() => setProfileDialog({ open: true, edit: p })}><Pencil size={13} /></IconButton>
                      <IconButton title="Duplicate (settings, favourites, media references — not projects)" onClick={() => { const n = window.prompt("Name for the copy", `${p.name} copy`); if (n) void act(() => api.duplicateProfile(p.id, n), "Profile duplicated"); }}><Copy size={13} /></IconButton>
                      <IconButton title="Export configuration (JSON)" onClick={() => exportConfig(p)}><Download size={13} /></IconButton>
                      {p.archived ? (
                        <>
                          <IconButton title="Restore" onClick={() => act(() => api.archiveProfile(p.id, false), "Profile restored")}><ArchiveRestore size={13} /></IconButton>
                          <IconButton title="Delete permanently" onClick={() => remove(p)}><Trash2 size={13} className="text-err" /></IconButton>
                        </>
                      ) : (
                        <IconButton title={p.id === active?.id ? "Switch away before archiving" : "Archive"} disabled={p.id === active?.id} onClick={() => act(() => api.archiveProfile(p.id, true), "Profile archived")}><Archive size={13} /></IconButton>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
        <p className="mt-3 text-[11px] leading-relaxed text-dim">
          Each profile has its own projects, export history, media references, favourite presets and research history — enforced by the Rust backend.
          Archiving hides a profile without removing anything; deleting requires archiving first and never deletes files on disk. Background jobs keep running when you switch profiles.
        </p>
      </div>
    </div>
  );
}
