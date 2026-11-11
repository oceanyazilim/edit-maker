import { AudioLines, Bookmark, Captions, FolderOpen, Layers, Library, Shuffle, Sparkles, Type, Wand2 } from "lucide-react";
import type { ReactNode } from "react";
import { useApp, type ToolId } from "../stores/app";
import { cx } from "../components/ui";

export const TOOLS: { id: ToolId; label: string; icon: ReactNode; disabled?: string }[] = [
  { id: "media", label: "Media", icon: <FolderOpen size={18} /> },
  { id: "auto", label: "Auto Edit", icon: <Wand2 size={18} /> },
  { id: "text", label: "Text", icon: <Type size={18} />, disabled: "Text rendering is not implemented yet" },
  { id: "transitions", label: "Transitions", icon: <Shuffle size={18} /> },
  { id: "effects", label: "Effects", icon: <Sparkles size={18} /> },
  { id: "overlays", label: "Overlays", icon: <Layers size={18} />, disabled: "Overlay opacity / blend modes are not implemented yet" },
  { id: "audio", label: "Audio", icon: <AudioLines size={18} /> },
  { id: "captions", label: "Captions", icon: <Captions size={18} />, disabled: "Captions need text rendering, which is not implemented yet" },
  { id: "library", label: "Library", icon: <Library size={18} /> },
  { id: "presets", label: "Presets", icon: <Bookmark size={18} /> },
];

export function ToolRail() {
  const tool = useApp((s) => s.leftTool);
  const leftOpen = useApp((s) => s.layout.left);
  const setTool = useApp((s) => s.setLeftTool);
  const toggleLayout = useApp((s) => s.toggleLayout);
  return (
    <nav aria-label="Tools" className="flex w-[66px] shrink-0 flex-col items-center gap-0.5 overflow-y-auto border-r border-line bg-surface py-2">
      {TOOLS.map((t) => {
        const active = leftOpen && tool === t.id;
        return (
          <button
            key={t.id}
            disabled={!!t.disabled}
            title={t.disabled ?? t.label}
            aria-pressed={active}
            onClick={() => (active ? toggleLayout("left", false) : setTool(t.id))}
            className={cx(
              "relative flex w-[58px] flex-col items-center gap-1 rounded-md py-2 text-[10px] transition-colors disabled:cursor-not-allowed disabled:opacity-30",
              active ? "bg-raised text-fg" : "text-muted hover:bg-raised/60 hover:text-fg",
            )}
          >
            {active && <span className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full bg-accent" />}
            <span className={active ? "text-accent" : undefined}>{t.icon}</span>
            <span className="leading-none">{t.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
