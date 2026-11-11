import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { api, dialogs } from "../services/api";
import { useApp } from "../stores/app";
import type { Profile } from "../types/studio";
import { Button, Dialog, Field, inputClass, Select } from "./ui";
import { ControlSliders } from "./ControlSliders";
import { defaultControls, PLATFORMS, VARIANTS } from "./NewProjectDialog";
import { Avatar } from "./ProfileSwitcher";

const COLORS = ["#3a3a42", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#ca8a04", "#16a34a", "#0891b2", "#2563eb"];

export const blankProfile = (): Profile => ({
  id: "",
  name: "",
  avatar: "",
  color: COLORS[1],
  description: "",
  primary_niche: null,
  niches: [],
  platforms: ["tiktok", "reels", "shorts"],
  channel_urls: [],
  branding: null,
  defaults: {
    preset_id: "high-energy-edm",
    variant: "balanced",
    duration_secs: 20,
    aspect: "9:16",
    fps: { num: 30, den: 1 },
    platform: "tiktok",
    controls: defaultControls(),
    export_encoder: "auto",
    export_quality: 18,
    export_dir: null,
    media_folder: null,
    naming: "{project}-{variant}-{n}",
  },
  archived: false,
  created_at: 0,
  updated_at: 0,
});

export function ProfileDialog() {
  const { profileDialog, setProfileDialog, profile: active, setProfile, toast, error } = useApp();
  const qc = useQueryClient();
  const presets = useQuery({ queryKey: ["presets"], queryFn: api.listPresets });
  const niches = useQuery({ queryKey: ["niches"], queryFn: api.listNiches });
  const [p, setP] = useState<Profile>(blankProfile());
  const [urls, setUrls] = useState("");
  const editing = !!profileDialog.edit;

  useEffect(() => {
    if (!profileDialog.open) return;
    const base = profileDialog.edit ?? blankProfile();
    setP(base);
    setUrls(base.channel_urls.join("\n"));
  }, [profileDialog]);

  const close = () => setProfileDialog({ open: false });
  const d = p.defaults;
  const setD = (patch: Partial<Profile["defaults"]>) => setP({ ...p, defaults: { ...d, ...patch } });

  const submit = async () => {
    const out: Profile = { ...p, channel_urls: urls.split(/\s+/).map((u) => u.trim()).filter(Boolean), avatar: p.avatar.trim().slice(0, 2) };
    try {
      const saved = editing ? await api.updateProfile(out) : await api.createProfile(out);
      if (active?.id === saved.id) setProfile(saved);
      await qc.invalidateQueries({ queryKey: ["profiles"] });
      toast({ kind: "success", title: editing ? "Profile updated" : `Profile "${saved.name}" created` });
      close();
    } catch (e) {
      error(e, "Profile not saved");
    }
  };

  const toggleNiche = (id: string) => setP({ ...p, niches: p.niches.includes(id) ? p.niches.filter((x) => x !== id) : [...p.niches, id] });
  const togglePlatform = (id: string) => setP({ ...p, platforms: p.platforms.includes(id) ? p.platforms.filter((x) => x !== id) : [...p.platforms, id] });

  return (
    <Dialog
      open={profileDialog.open}
      onClose={close}
      title={editing ? `Profile settings — ${profileDialog.edit?.name}` : "New profile"}
      width={720}
      footer={
        <>
          <Button variant="ghost" onClick={close}>Cancel</Button>
          <Button variant="primary" disabled={!p.name.trim()} onClick={submit}>{editing ? "Save changes" : "Create profile"}</Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3">
        <div className="col-span-2 flex items-end gap-3">
          <Avatar p={{ ...p, name: p.name || "?" }} size={44} />
          <Field label="Profile name">
            <input className={inputClass + " w-64"} value={p.name} maxLength={60} onChange={(e) => setP({ ...p, name: e.target.value })} placeholder="e.g. AnimeMotion" autoFocus />
          </Field>
          <Field label="Icon (1–2 chars / emoji)">
            <input className={inputClass + " w-24"} value={p.avatar} onChange={(e) => setP({ ...p, avatar: e.target.value })} />
          </Field>
          <div className="flex gap-1 pb-1">
            {COLORS.map((c) => (
              <button key={c} aria-label={`Colour ${c}`} onClick={() => setP({ ...p, color: c })} className="h-5 w-5 rounded" style={{ background: c, outline: p.color === c ? "2px solid white" : "none" }} />
            ))}
          </div>
        </div>
        <div className="col-span-2">
          <Field label="Description">
            <input className={inputClass + " w-full"} value={p.description} onChange={(e) => setP({ ...p, description: e.target.value })} />
          </Field>
        </div>
        <Field label="Primary niche">
          <select className={inputClass} value={p.primary_niche ?? ""} onChange={(e) => { const id = e.target.value || null; const n = niches.data?.find((x) => x.id === id); setP({ ...p, primary_niche: id, defaults: n ? { ...d, preset_id: n.default_preset } : d }); }}>
            <option value="">General</option>
            {(niches.data ?? []).map((n) => <option key={n.id} value={n.id}>{n.category} · {n.name}</option>)}
          </select>
        </Field>
        <Field label="Default editing preset">
          <Select value={d.preset_id} onChange={(v) => setD({ preset_id: v })} options={(presets.data ?? []).map((x) => ({ value: x.id, label: x.name }))} />
        </Field>
        <div className="col-span-2">
          <Field label="Additional niches">
            <div className="flex max-h-24 flex-wrap gap-1 overflow-y-auto rounded-md border border-line p-1.5">
              {(niches.data ?? []).map((n) => (
                <button key={n.id} onClick={() => toggleNiche(n.id)} className={`rounded px-1.5 py-0.5 text-[11px] ${p.niches.includes(n.id) ? "bg-accent text-white" : "bg-raised text-muted"}`}>{n.name}</button>
              ))}
            </div>
          </Field>
        </div>
        <Field label="Target platforms">
          <div className="flex flex-wrap gap-1">
            {Object.entries(PLATFORMS).map(([k, v]) => (
              <button key={k} onClick={() => togglePlatform(k)} className={`rounded px-1.5 py-0.5 text-[11px] ${p.platforms.includes(k) ? "bg-accent text-white" : "bg-raised text-muted"}`}>{v.label}</button>
            ))}
          </div>
        </Field>
        <Field label="Channel URLs (optional, one per line)" hint="Stored locally; no account is connected.">
          <textarea className={inputClass + " h-14 py-1"} value={urls} onChange={(e) => setUrls(e.target.value)} />
        </Field>
        <Field label="Default format">
          <Select value={d.platform} onChange={(v) => setD({ platform: v, aspect: PLATFORMS[v].aspect, duration_secs: d.duration_secs })} options={Object.entries(PLATFORMS).map(([k, v]) => ({ value: k, label: `${v.label} (${v.aspect})` }))} />
        </Field>
        <div className="grid grid-cols-3 gap-2">
          <Field label="Duration (s)">
            <input className={inputClass} type="number" min={3} max={600} value={d.duration_secs} onChange={(e) => setD({ duration_secs: Math.max(3, Math.min(600, Number(e.target.value) || 20)) })} />
          </Field>
          <Field label="FPS">
            <Select value={String(d.fps.num)} onChange={(v) => setD({ fps: { num: Number(v), den: 1 } })} options={["24", "25", "30", "60"].map((f) => ({ value: f, label: f }))} />
          </Field>
          <Field label="Variant">
            <Select value={d.variant} onChange={(v) => setD({ variant: v })} options={VARIANTS} />
          </Field>
        </div>
        <Field label="Export folder" hint="Empty = project's exports folder.">
          <div className="flex gap-1">
            <input className={inputClass + " flex-1"} readOnly value={d.export_dir ?? ""} placeholder="(project folder)" />
            <Button icon={<FolderOpen size={13} />} onClick={async () => setD({ export_dir: await dialogs.pickFolder() })} />
          </div>
        </Field>
        <Field label="File naming" hint="{project} {variant} {profile} {date} {n}">
          <input className={inputClass} value={d.naming} onChange={(e) => setD({ naming: e.target.value })} />
        </Field>
        <Field label="Default media folder">
          <div className="flex gap-1">
            <input className={inputClass + " flex-1"} readOnly value={d.media_folder ?? ""} placeholder="(none)" />
            <Button icon={<FolderOpen size={13} />} onClick={async () => setD({ media_folder: await dialogs.pickFolder() })} />
          </div>
        </Field>
        <Field label="Branding text (optional)">
          <input className={inputClass} value={p.branding ?? ""} onChange={(e) => setP({ ...p, branding: e.target.value || null })} />
        </Field>
        <div className="col-span-2 rounded-lg border border-line p-3">
          <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-dim">Default creative controls</div>
          <ControlSliders value={d.controls} onChange={(c) => setD({ controls: c })} />
        </div>
      </div>
      <p className="mt-3 text-[11px] text-dim">New projects in this profile start from these defaults. Changing a project's settings never changes the profile defaults.</p>
    </Dialog>
  );
}
