// Compact UI primitives in the spirit of shadcn/ui (owned source, Tailwind tokens).
import { useEffect, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { ChevronDown, ChevronRight, X } from "lucide-react";

const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");
export { cx };

type Variant = "default" | "primary" | "ghost" | "danger" | "outline";
export function Button({
  variant = "default",
  size = "md",
  className,
  icon,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md"; icon?: ReactNode }) {
  const v: Record<Variant, string> = {
    default: "bg-raised hover:bg-overlay border border-line text-fg",
    primary: "bg-accent text-accent-fg hover:bg-accent-hover font-medium",
    ghost: "hover:bg-raised text-muted hover:text-fg",
    danger: "bg-raised hover:bg-err/20 border border-line text-err",
    outline: "border border-line-strong hover:bg-raised text-fg",
  };
  return (
    <button
      className={cx(
        "inline-flex items-center justify-center gap-1.5 rounded-md transition-colors disabled:opacity-40 disabled:pointer-events-none whitespace-nowrap",
        size === "sm" ? "h-7 px-2 text-[11px]" : "h-8 px-3 text-xs",
        v[variant],
        className,
      )}
      {...rest}
    >
      {icon}
      {children}
    </button>
  );
}

export function IconButton({ title, children, active, className, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { active?: boolean }) {
  return (
    <button
      title={title}
      aria-label={title}
      className={cx(
        "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors disabled:opacity-30",
        active ? "bg-overlay text-fg" : "text-muted hover:bg-raised hover:text-fg",
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

export function Panel({ title, actions, children, className, bodyClass }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string; bodyClass?: string }) {
  return (
    <section className={cx("flex min-h-0 flex-col rounded-lg border border-line bg-surface", className)}>
      {(title || actions) && (
        <header className="flex h-9 shrink-0 items-center justify-between border-b border-line px-3">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">{title}</h3>
          <div className="flex items-center gap-1">{actions}</div>
        </header>
      )}
      <div className={cx("min-h-0 flex-1", bodyClass ?? "overflow-auto p-3")}>{children}</div>
    </section>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[11px] font-medium text-muted">{label}</span>
      {children}
      {hint && <span className="text-[11px] text-dim">{hint}</span>}
    </label>
  );
}

export const inputClass =
  "h-8 rounded-md border border-line bg-raised px-2 text-xs text-fg outline-none focus:border-accent placeholder:text-dim";

export function Select<T extends string>({ value, onChange, options, className }: { value: T; onChange: (v: T) => void; options: { value: T; label: string }[]; className?: string }) {
  return (
    <select className={cx(inputClass, "pr-6", className)} value={value} onChange={(e) => onChange(e.target.value as T)}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Slider({ label, value, onChange, hint, min = 0, max = 1, step = 0.01, format }: { label: string; value: number; onChange: (v: number) => void; hint?: string; min?: number; max?: number; step?: number; format?: (v: number) => string }) {
  return (
    <div className="flex flex-col gap-1" title={hint}>
      <div className="flex items-center justify-between text-[11px]">
        <span className="text-muted">{label}</span>
        <span className="tabular text-dim">{format ? format(value) : value.toFixed(2)}</span>
      </div>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(parseFloat(e.target.value))} className="h-1 w-full cursor-pointer" />
    </div>
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "ok" | "warn" | "err" | "accent" }) {
  const t = {
    neutral: "bg-raised text-muted border-line",
    ok: "bg-ok/10 text-ok border-ok/20",
    warn: "bg-warn/10 text-warn border-warn/20",
    err: "bg-err/10 text-err border-err/20",
    accent: "bg-accent/10 text-accent border-accent/25",
  }[tone];
  return <span className={cx("inline-flex items-center rounded border px-1.5 py-px text-[10px] font-medium uppercase tracking-wide", t)}>{children}</span>;
}

export function Progress({ value, className }: { value?: number | null; className?: string }) {
  return (
    <div className={cx("h-1 overflow-hidden rounded bg-raised", className)}>
      {value == null ? (
        <div className="h-full w-1/3 animate-pulse rounded bg-muted" />
      ) : (
        <div className="h-full rounded bg-accent transition-[width] duration-200" style={{ width: `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%` }} />
      )}
    </div>
  );
}

export function EmptyState({ icon, title, body, action }: { icon?: ReactNode; title: string; body?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center">
      {icon && <div className="text-dim">{icon}</div>}
      <div className="text-sm font-medium text-fg">{title}</div>
      {body && <div className="max-w-sm text-xs leading-relaxed text-muted">{body}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Dialog({ open, onClose, title, children, footer, width = 560 }: { open: boolean; onClose: () => void; title: string; children: ReactNode; footer?: ReactNode; width?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    ref.current?.focus();
    return () => window.removeEventListener("keydown", k);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onMouseDown={onClose}>
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="flex max-h-[88vh] flex-col rounded-xl border border-line bg-surface shadow-2xl outline-none"
        style={{ width }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="flex h-11 items-center justify-between border-b border-line px-4">
          <h2 className="text-sm font-semibold">{title}</h2>
          <IconButton title="Close" onClick={onClose}>
            <X size={15} />
          </IconButton>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-4">{children}</div>
        {footer && <footer className="flex justify-end gap-2 border-t border-line px-4 py-3">{footer}</footer>}
      </div>
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded border border-line bg-raised px-1 font-mono text-[10px] text-muted">{children}</kbd>;
}

export function Stat({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[11px] text-dim">{label}</span>
      <span className="tabular text-sm font-medium">{value}</span>
      {sub && <span className="text-[11px] text-muted">{sub}</span>}
    </div>
  );
}

/** Segmented control (mutually exclusive buttons). */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  className,
  size = "md",
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode; disabled?: boolean; title?: string }[];
  className?: string;
  size?: "sm" | "md";
}) {
  return (
    <div role="radiogroup" className={cx("flex rounded-md border border-line bg-base p-0.5", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={o.value === value}
          disabled={o.disabled}
          title={o.title}
          onClick={() => onChange(o.value)}
          className={cx(
            "flex-1 whitespace-nowrap rounded px-2 transition-colors disabled:cursor-not-allowed disabled:opacity-35",
            size === "sm" ? "h-6 text-[11px]" : "h-7 text-xs",
            o.value === value ? "bg-overlay text-fg shadow-sm ring-1 ring-line-strong" : "text-muted hover:text-fg",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Underlined tab strip. */
export function Tabs<T extends string>({ value, onChange, tabs, className }: { value: T; onChange: (v: T) => void; tabs: { value: T; label: ReactNode; disabled?: boolean; title?: string }[]; className?: string }) {
  return (
    <div role="tablist" className={cx("flex h-9 shrink-0 items-stretch gap-3 border-b border-line px-3", className)}>
      {tabs.map((t) => (
        <button
          key={t.value}
          role="tab"
          aria-selected={t.value === value}
          disabled={t.disabled}
          title={t.title}
          onClick={() => onChange(t.value)}
          className={cx(
            "-mb-px border-b-2 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-35",
            t.value === value ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg",
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

export function Switch({ checked, onChange, disabled, label, hint }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: ReactNode; hint?: string }) {
  return (
    <label className={cx("flex items-center justify-between gap-3 text-xs", disabled ? "cursor-not-allowed opacity-45" : "cursor-pointer")} title={hint}>
      <span className="text-muted">{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cx("relative h-4 w-7 shrink-0 rounded-full transition-colors", checked ? "bg-accent" : "bg-line-strong")}
      >
        <span className={cx("absolute top-0.5 h-3 w-3 rounded-full bg-white transition-all", checked ? "left-3.5" : "left-0.5")} />
      </button>
    </label>
  );
}

/** Collapsible panel section with a small caps header. */
export function Section({ title, children, defaultOpen = true, actions, className }: { title: ReactNode; children: ReactNode; defaultOpen?: boolean; actions?: ReactNode; className?: string }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={cx("border-b border-line", className)}>
      <header className="flex h-8 items-center justify-between px-3">
        <button className="flex min-w-0 items-center gap-1 text-[11px] font-semibold uppercase tracking-wider text-muted hover:text-fg" onClick={() => setOpen(!open)} aria-expanded={open}>
          {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span className="truncate">{title}</span>
        </button>
        <div className="flex items-center gap-1">{actions}</div>
      </header>
      {open && <div className="px-3 pb-3">{children}</div>}
    </section>
  );
}

/** Header for a left tool panel. */
export function PanelHeader({ title, actions }: { title: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex h-10 shrink-0 items-center justify-between border-b border-line px-3">
      <h2 className="text-[13px] font-semibold">{title}</h2>
      <div className="flex items-center gap-1">{actions}</div>
    </header>
  );
}
