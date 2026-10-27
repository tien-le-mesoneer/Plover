import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { SPLIT_MAX, SPLIT_MIN, useLayout, type Orientation } from "../store/layout";

const STEP = 20;

/**
 * The handle between the request and response panes. Dragging sets the request pane's
 * share of the split area; arrow keys move it by 2 %, Home/End go to 20 %/80 %, and Enter
 * or a double-click goes back to half. Only the shown orientation's ratio changes.
 */
export function PaneResizer({ permille, orientation }: { permille: number; orientation: Orientation }) {
  const setSplitPermille = useLayout((s) => s.setSplitPermille);
  const dragging = useRef(false);
  const side = orientation === "side";

  const fromPointer = (e: PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.parentElement?.getBoundingClientRect();
    if (!box) return;
    const size = side ? box.width : box.height;
    if (size <= 0) return;
    const offset = side ? e.clientX - box.left : e.clientY - box.top;
    setSplitPermille(orientation, (offset / size) * 1000);
  };
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    dragging.current = true;
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (dragging.current) fromPointer(e);
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    dragging.current = false;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const next =
      e.key === "ArrowLeft" || e.key === "ArrowUp"
        ? permille - STEP
        : e.key === "ArrowRight" || e.key === "ArrowDown"
          ? permille + STEP
          : e.key === "Home"
            ? SPLIT_MIN
            : e.key === "End"
              ? SPLIT_MAX
              : undefined;
    if (e.key === "Enter") {
      e.preventDefault();
      setSplitPermille(orientation, null);
      return;
    }
    if (next === undefined) return;
    e.preventDefault();
    setSplitPermille(orientation, next);
  };

  return (
    <div
      className="pane-resizer"
      role="separator"
      aria-orientation={side ? "vertical" : "horizontal"}
      aria-label="Resize request and response"
      aria-valuenow={permille}
      aria-valuemin={SPLIT_MIN}
      aria-valuemax={SPLIT_MAX}
      aria-valuetext={`Request ${Math.round(permille / 10)} %`}
      title="Drag to resize · double-click to reset"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => setSplitPermille(orientation, null)}
      onKeyDown={onKeyDown}
    />
  );
}
