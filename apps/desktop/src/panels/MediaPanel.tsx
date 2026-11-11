import { Film, FolderInput, ImageIcon, Music, Plus, Search, Trash2, Upload } from "lucide-react";
import { useMemo, useState } from "react";
import { api, fileUrl } from "../services/api";
import { useApp } from "../stores/app";
import type { MediaAsset } from "../types/studio";
import { fmtSeconds, ticksToSeconds } from "../lib/time";
import { Badge, Button, EmptyState, IconButton, inputClass, PanelHeader, Progress, Segmented, cx } from "../components/ui";
import { cmd } from "../shell/commands";

type Filter = "all" | "video" | "audio" | "image";

export function MediaPanel() {
  const project = useApp((s) => s.project);
  const jobs = useApp((s) => s.jobs);
  const [filter, setFilter] = useState<Filter>("all");
  const [q, setQ] = useState("");
  const usage = useMemo(() => {
    const m = new Map<string, number>();
    project?.timeline.tracks.forEach((t) => t.clips.forEach((c) => m.set(c.asset_id, (m.get(c.asset_id) ?? 0) + 1)));
    return m;
  }, [project]);
  if (!project) return null;
  const importing = Object.values(jobs).some((j) => j.kind === "import" && (j.status === "running" || j.status === "queued"));
  const list = project.assets.filter((a) => (filter === "all" || a.kind === filter) && a.name.toLowerCase().includes(q.toLowerCase()));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PanelHeader
        title="Media"
        actions={
          <>
            <IconButton title="Import a folder of clips" onClick={cmd.importFolder}>
              <FolderInput size={14} />
            </IconButton>
            <IconButton title="Import music" onClick={cmd.importMusic}>
              <Music size={14} />
            </IconButton>
            <Button size="sm" variant="primary" icon={<Upload size={12} />} onClick={cmd.importFootage}>
              Import
            </Button>
          </>
        }
      />
      <div className="flex flex-col gap-2 border-b border-line p-2">
        <div className="relative">
          <Search size={12} className="absolute left-2 top-2.5 text-dim" />
          <input className={inputClass + " h-7 w-full pl-6"} placeholder="Search media…" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <Segmented
          size="sm"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: `All ${project.assets.length}` },
            { value: "video", label: "Video" },
            { value: "audio", label: "Audio" },
            { value: "image", label: "Image" },
          ]}
        />
        {importing && <Progress value={null} />}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {list.length === 0 ? (
          <EmptyState
            icon={<Film size={22} />}
            title={project.assets.length ? "No matching media" : "No media yet"}
            body="Import licensed clips and the project's music. Scenes and beats are analyzed locally."
            action={!project.assets.length && <Button onClick={cmd.importFootage}>Import media</Button>}
          />
        ) : (
          <div className="grid grid-cols-2 gap-2">
            {list.map((a) => (
              <AssetCard key={a.id} a={a} used={usage.get(a.id) ?? 0} />
            ))}
          </div>
        )}
      </div>
      <div className="border-t border-line px-3 py-1.5 text-[10px] text-dim">Double-click or + adds a clip at the playhead.</div>
    </div>
  );
}

function AssetCard({ a, used }: { a: MediaAsset; used: number }) {
  const patchProject = useApp((s) => s.patchProject);
  const error = useApp((s) => s.error);
  const remove = async () => {
    if (used && !window.confirm(`"${a.name}" is used by ${used} clip(s) on the timeline. Remove it from the project anyway?`)) return;
    try {
      patchProject(await api.removeAsset(a.id));
    } catch (e) {
      error(e, "Could not remove media");
    }
  };
  return (
    <div className="cv-auto group relative overflow-hidden rounded-md border border-line bg-raised" title={a.path} onDoubleClick={() => a.role !== "music" && cmd.insertAsset(a.id)}>
      {a.thumbnail ? (
        <img src={fileUrl(a.thumbnail)} className="aspect-video w-full object-cover" alt="" draggable={false} />
      ) : (
        <div className="flex aspect-video w-full items-center justify-center bg-base text-dim">{a.kind === "audio" ? <Music size={18} /> : a.kind === "image" ? <ImageIcon size={18} /> : <Film size={18} />}</div>
      )}
      <div className="flex items-center gap-1 px-1.5 py-1">
        <span className="min-w-0 flex-1 truncate text-[10px] text-muted">{a.name}</span>
        {used > 0 && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" title={`Used ${used}× on the timeline`} />}
      </div>
      <span className="tabular absolute right-1 top-1 rounded bg-black/70 px-1 text-[9px] text-white/85">{a.kind === "image" ? "image" : fmtSeconds(ticksToSeconds(a.metadata.duration), 1)}</span>
      {a.role === "music" && (
        <span className="absolute left-1 top-1">
          <Badge tone="accent">music</Badge>
        </span>
      )}
      {a.missing && (
        <span className="absolute left-1 bottom-7">
          <Badge tone="err">missing</Badge>
        </span>
      )}
      <div className={cx("absolute inset-x-0 top-0 hidden justify-end gap-0.5 p-1 group-hover:flex", a.role === "music" ? "pt-6" : "pt-6")}>
        {a.role !== "music" && (
          <button className="rounded bg-black/75 p-1 text-white/85 hover:text-accent" title="Add at playhead" onClick={() => cmd.insertAsset(a.id)}>
            <Plus size={12} />
          </button>
        )}
        <button className="rounded bg-black/75 p-1 text-white/85 hover:text-err" title="Remove from project" onClick={remove}>
          <Trash2 size={12} />
        </button>
      </div>
    </div>
  );
}
