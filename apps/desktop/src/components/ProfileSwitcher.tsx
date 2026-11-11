import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronDown, Plus, Search, Settings2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../services/api";
import { useApp } from "../stores/app";
import type { Profile } from "../types/studio";
import { cx, inputClass } from "./ui";

export function Avatar({ p, size = 26 }: { p: Pick<Profile, "avatar" | "color" | "name">; size?: number }) {
  return (
    <span className="inline-flex shrink-0 items-center justify-center rounded-md font-semibold text-white" style={{ width: size, height: size, background: p.color, fontSize: size * 0.45 }} aria-hidden>
      {p.avatar || p.name.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** Switch workspace: saves (or keeps an autosave of) the open project, never cancels jobs. */
export async function switchProfileFlow(target: Profile, qc: ReturnType<typeof useQueryClient>) {
  const st = useApp.getState();
  if (st.profile?.id === target.id) return;
  let save = false;
  if (st.project) {
    save = window.confirm(`Switch to "${target.name}"?\n\nOK: save "${st.project.name}" and switch.\nCancel: stay on the current profile.`);
    if (!save) return;
  }
  try {
    const p = await api.switchProfile(target.id, save);
    st.setProject(null);
    st.selectClip(null);
    st.setProfile(p);
    st.setRoute("home");
    // Profile-scoped server state must be reloaded.
    await qc.invalidateQueries();
    st.toast({ kind: "info", title: `Switched to ${p.name}` });
  } catch (e) {
    st.error(e, "Could not switch profile");
  }
}

export function ProfileSwitcher({ compact = false }: { compact?: boolean }) {
  const profile = useApp((s) => s.profile);
  const setProfile = useApp((s) => s.setProfile);
  const setProfileDialog = useApp((s) => s.setProfileDialog);
  const setRoute = useApp((s) => s.setRoute);
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const profiles = useQuery({ queryKey: ["profiles", q], queryFn: () => api.listProfiles(q, false), enabled: open });
  const nicheList = useQuery({ queryKey: ["niches"], queryFn: api.listNiches });

  useEffect(() => {
    void api.activeProfile().then(setProfile).catch(() => undefined);
  }, [setProfile]);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open]);

  if (!profile) return null;
  const nicheName = (id?: string | null) => nicheList.data?.find((n) => n.id === id)?.name ?? (id ? id : "General");

  return (
    <div ref={ref} className="relative">
      <button onClick={() => setOpen(!open)} className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-raised" aria-haspopup="menu" aria-expanded={open} title={`Profile: ${profile.name} (click to switch)`}>
        <Avatar p={profile} size={compact ? 24 : 26} />
        <div className={cx("text-left leading-tight", compact && "hidden xl:block")}>
          <div className="max-w-32 truncate text-xs font-medium">{profile.name}</div>
          {!compact && <div className="max-w-40 truncate text-[10px] text-dim">{nicheName(profile.primary_niche)}</div>}
        </div>
        <ChevronDown size={14} className="text-dim" />
      </button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-50 mt-1 w-72 rounded-lg border border-line bg-surface shadow-2xl">
          <div className="relative border-b border-line p-2">
            <Search size={13} className="absolute left-4 top-4.5 text-dim" />
            <input autoFocus className={inputClass + " w-full pl-7"} placeholder="Search profiles…" value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
          <div className="max-h-72 overflow-y-auto p-1">
            {(profiles.data ?? []).map((p) => (
              <button
                key={p.id}
                role="menuitemradio"
                aria-checked={p.id === profile.id}
                onClick={() => {
                  setOpen(false);
                  void switchProfileFlow(p, qc);
                }}
                className={cx("flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-raised", p.id === profile.id && "bg-raised")}
              >
                <Avatar p={p} size={24} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-xs">{p.name}</div>
                  <div className="truncate text-[10px] text-dim">{nicheName(p.primary_niche)}</div>
                </div>
                {p.id === profile.id && <Check size={14} className="text-accent" />}
              </button>
            ))}
            {profiles.data?.length === 0 && <div className="p-3 text-xs text-dim">No matching profiles.</div>}
          </div>
          <div className="flex flex-col border-t border-line p-1">
            <button className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-raised" onClick={() => { setOpen(false); setProfileDialog({ open: true, edit: null }); }}>
              <Plus size={14} /> New profile
            </button>
            <button className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-raised" onClick={() => { setOpen(false); setProfileDialog({ open: true, edit: profile }); }}>
              <Settings2 size={14} /> Profile settings
            </button>
            <button className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-raised" onClick={() => { setOpen(false); setRoute("profiles"); }}>
              <Settings2 size={14} /> Manage profiles
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
