import type { ReactNode } from "react";

/**
 * Small buttons in the top-right corner of a box (its parent has class `has-corner`).
 * They show while the pointer is over the box or focus is inside it, and always on
 * touch screens; they stay in the Tab order. `note` keeps them shown while it is set.
 */
export function CornerActions({ children, note }: { children: ReactNode; note?: string | null }) {
  return <div className={`corner-actions ${note ? "has-note" : ""}`.trim()}>{children}</div>;
}
