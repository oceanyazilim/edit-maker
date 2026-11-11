import { Clapperboard } from "lucide-react";
import { useApp } from "../stores/app";
import { Button, EmptyState } from "../components/ui";
import { Preview } from "../editor/Preview";
import { Timeline } from "../editor/Timeline";
import { Inspector } from "../editor/Inspector";
import { useResizable } from "../editor/useResizable";
import { ToolRail, TOOLS } from "../shell/ToolRail";
import { AutoEditPanel } from "../panels/AutoEditPanel";
import { MediaPanel } from "../panels/MediaPanel";
import { TransitionsPanel } from "../panels/TransitionsPanel";
import { EffectsPanel } from "../panels/EffectsPanel";
import { AudioPanel } from "../panels/AudioPanel";
import { LibraryPanel } from "../panels/LibraryPanel";
import { PresetsPanel } from "../panels/PresetsPanel";
import { openProjectFlow } from "./Projects";

/** Five-region workstation: tool rail · tool panel · preview · inspector · timeline. */
export default function Workstation() {
  const project = useApp((s) => s.project);
  const layout = useApp((s) => s.layout);
  const tool = useApp((s) => s.leftTool);
  const left = useResizable("ws-left", 310, 260, 480, "x");
  const right = useResizable("ws-right", 300, 260, 440, "x", true);
  const bottom = useResizable("ws-bottom", 290, 180, 600, "y", true);

  if (!project)
    return (
      <EmptyState
        icon={<Clapperboard size={28} />}
        title="No project open"
        body="Create a project or open an existing one to start editing."
        action={
          <div className="flex gap-2">
            <Button variant="primary" onClick={() => useApp.getState().setNewProjectOpen(true)}>
              New project
            </Button>
            <Button onClick={() => openProjectFlow()}>Open…</Button>
          </div>
        }
      />
    );

  const panel = (() => {
    switch (tool) {
      case "media":
        return <MediaPanel />;
      case "auto":
        return <AutoEditPanel />;
      case "transitions":
        return <TransitionsPanel />;
      case "effects":
        return <EffectsPanel />;
      case "audio":
        return <AudioPanel />;
      case "library":
        return <LibraryPanel />;
      case "presets":
        return <PresetsPanel />;
      default: {
        const t = TOOLS.find((x) => x.id === tool);
        return <EmptyState title={t?.label ?? "Unavailable"} body={t?.disabled ?? "Not available."} />;
      }
    }
  })();

  return (
    <div className="flex h-full min-h-0">
      <ToolRail />
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-h-0 flex-1">
          {layout.left && (
            <>
              <aside style={{ width: left.size }} className="flex min-h-0 shrink-0 flex-col border-r border-line bg-surface">
                {panel}
              </aside>
              <div className="resize-x" onMouseDown={left.onMouseDown} title="Drag to resize" />
            </>
          )}
          <section className="min-w-0 flex-1 bg-base">
            <Preview />
          </section>
          {layout.right && (
            <>
              <div className="resize-x" onMouseDown={right.onMouseDown} title="Drag to resize" />
              <aside style={{ width: right.size }} className="flex min-h-0 shrink-0 flex-col border-l border-line bg-surface">
                <Inspector />
              </aside>
            </>
          )}
        </div>
        {layout.bottom && (
          <>
            <div className="resize-y" onMouseDown={bottom.onMouseDown} title="Drag to resize" />
            <section style={{ height: bottom.size }} className="shrink-0 border-t border-line bg-surface">
              <Timeline />
            </section>
          </>
        )}
      </div>
    </div>
  );
}
