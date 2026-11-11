import { useCallback, useRef, useState } from "react";

/** Drag-resizable panel size (px), persisted per key in localStorage. */
export function useResizable(key: string, initial: number, min: number, max: number, axis: "x" | "y", invert = false) {
  const read = () => {
    try {
      const v = Number(localStorage.getItem(`panel:${key}`));
      return v >= min && v <= max ? v : initial;
    } catch {
      return initial;
    }
  };
  const [size, setSize] = useState<number>(read);
  const start = useRef<{ pos: number; size: number } | null>(null);
  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      start.current = { pos: axis === "x" ? e.clientX : e.clientY, size };
      const move = (ev: MouseEvent) => {
        if (!start.current) return;
        const d = (axis === "x" ? ev.clientX : ev.clientY) - start.current.pos;
        const next = Math.max(min, Math.min(max, start.current.size + (invert ? -d : d)));
        setSize(next);
        try {
          localStorage.setItem(`panel:${key}`, String(next));
        } catch {
          /* storage unavailable */
        }
      };
      const up = () => {
        start.current = null;
        window.removeEventListener("mousemove", move);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up, { once: true });
    },
    [axis, invert, key, max, min, size],
  );
  return { size, onMouseDown };
}
