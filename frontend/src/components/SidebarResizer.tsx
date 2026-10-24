import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { effectiveSidebarWidth, SIDEBAR_MIN, useLayout } from "../store/layout";

const STEP = 16;

/** The current window width (re-read by the caller on `resize`). */
function windowWidth(): number {
  return window.innerWidth;
}

/**
 * The drag handle on the sidebar's right edge. Arrow keys move it by 16 px, Home/End go
 * to the smallest/largest width, and Enter or a double-click goes back to the default.
 */
export function SidebarResizer({ width }: { width: number }) {
  const setSidebarWidth = useLayout((s) => s.setSidebarWidth);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);
  const max = Math.max(SIDEBAR_MIN, Math.floor(windowWidth() / 2));

  const set = (px: number) => setSidebarWidth(effectiveSidebarWidth(px, windowWidth()));

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    drag.current = { startX: e.clientX, startWidth: width };
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    set(drag.current.startWidth + e.clientX - drag.current.startX);
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    drag.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const next =
      e.key === "ArrowLeft"
        ? width - STEP
        : e.key === "ArrowRight"
          ? width + STEP
          : e.key === "Home"
            ? SIDEBAR_MIN
            : e.key === "End"
              ? max
              : null;
    if (e.key === "Enter") {
      e.preventDefault();
      setSidebarWidth(null);
      return;
    }
    if (next === null) return;
    e.preventDefault();
    set(next);
  };

  return (
    <div
      className="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuenow={width}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={max}
      title="Drag to resize · double-click to reset"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => setSidebarWidth(null)}
      onKeyDown={onKeyDown}
    />
  );
}
