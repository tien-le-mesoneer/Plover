import type { KeyboardEvent } from "react";
import { useLayout, type Orientation } from "../../store/layout";
import { SplitSideIcon, SplitStackedIcon } from "../icons";

const NARROW = "Too narrow to place side by side";

/**
 * Switches the request and response panes between side by side and stacked. While the
 * split area is too narrow (`forced`) it shows the stacked state, stays focusable with
 * its reason as the tooltip, and does nothing.
 */
export function SplitToggle({ orientation, forced }: { orientation: Orientation; forced: boolean }) {
  const setSplitOrientation = useLayout((s) => s.setSplitOrientation);
  const stacked = orientation === "stacked";
  const label = stacked ? "Place request and response side by side" : "Stack request and response";
  const flip = () => {
    if (!forced) setSplitOrientation(stacked ? "side" : "stacked");
  };
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      flip();
    }
  };
  return (
    <button
      type="button"
      className="icon split-toggle"
      aria-label={label}
      aria-pressed={stacked}
      aria-disabled={forced ? true : undefined}
      title={forced ? NARROW : label}
      onClick={flip}
      onKeyDown={onKeyDown}
    >
      {stacked ? <SplitSideIcon /> : <SplitStackedIcon />}
    </button>
  );
}
