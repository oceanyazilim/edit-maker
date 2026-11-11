import { Layers, Megaphone, TrendingUp } from "lucide-react";
import { Badge, Panel } from "../components/ui";

// Honest status pages: no fake data, no buttons that pretend to work.
const INFO = {
  trends: {
    icon: <TrendingUp size={20} />,
    title: "Trend Intelligence",
    phase: "Phase 4",
    body: "Trend data will only come from documented, authorized sources. No provider is connected, so no trend data is shown.",
    rows: [
      ["YouTube Data API", "Requires an API key (user-supplied). Not configured."],
      ["TikTok API for Business / Discovery", "Requires approved developer access. Not configured."],
      ["Meta / Instagram Graph API", "Requires a connected business account. Not configured."],
      ["TikTok Creative Center", "Manual research reference only (not an API)."],
    ],
  },
  batch: {
    icon: <Layers size={20} />,
    title: "Batch Studio",
    phase: "Phase 6",
    body: "The engine already supports deterministic variants and an edit-similarity metric; the queue-based batch UI is not built yet.",
    rows: [["Variant similarity metric", "Implemented in studio-planner (similarity.rs)"], ["Batch queue UI", "Not implemented"]],
  },
  campaigns: {
    icon: <Megaphone size={20} />,
    title: "Campaign Studio",
    phase: "Phase 6",
    body: "Campaign grouping, content status and authorized analytics are planned. Local exports will never be shown as 'published'.",
    rows: [["Campaign model", "Not implemented"]],
  },
} as const;

export default function Planned({ which }: { which: keyof typeof INFO }) {
  const i = INFO[which];
  return (
    <div className="h-full overflow-y-auto p-5">
      <div className="mx-auto max-w-2xl">
        <Panel title={i.title} actions={<Badge>{i.phase}</Badge>}>
          <div className="flex gap-3">
            <div className="text-dim">{i.icon}</div>
            <p className="text-xs leading-relaxed text-muted">{i.body}</p>
          </div>
          <table className="mt-4 w-full text-xs">
            <tbody>
              {i.rows.map(([k, v]) => (
                <tr key={k} className="border-t border-line">
                  <td className="py-1.5 pr-3">{k}</td>
                  <td className="text-dim">{v}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      </div>
    </div>
  );
}
