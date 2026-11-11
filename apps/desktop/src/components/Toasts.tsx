import { X } from "lucide-react";
import { useApp } from "../stores/app";
import { cx } from "./ui";

export function Toasts() {
  const { toasts, dismiss } = useApp();
  return (
    <div className="fixed left-1/2 top-14 z-50 flex w-[440px] -translate-x-1/2 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.kind === "error" ? "alert" : "status"}
          className={cx("rounded-lg border bg-surface px-3 py-2 shadow-xl", t.kind === "error" ? "border-err/40" : t.kind === "success" ? "border-ok/30" : "border-line")}
        >
          <div className="flex items-start justify-between gap-2">
            <span className={cx("text-xs", t.kind === "error" ? "text-err" : "text-fg")}>{t.title}</span>
            <button aria-label="Dismiss" onClick={() => dismiss(t.id)} className="text-dim hover:text-fg">
              <X size={13} />
            </button>
          </div>
          {t.details && (
            <details className="mt-1">
              <summary className="cursor-pointer text-[11px] text-dim">Details</summary>
              <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap text-[11px] text-muted">{t.details}</pre>
            </details>
          )}
        </div>
      ))}
    </div>
  );
}
