import type { ReactNode } from "react";

interface Props {
  kind: "warn" | "error";
  role?: "alert" | "note" | "status";
  children: ReactNode;
  /** Buttons shown after the message. */
  actions?: ReactNode;
}

/** A message bar. The text sits in one element so inline `<code>` and text
 *  nodes flow as a paragraph instead of becoming separate flex items. */
export function Banner({ kind, role, children, actions }: Props) {
  return (
    <div className={`banner ${kind}`} role={role}>
      <div className="banner-text">{children}</div>
      {actions}
    </div>
  );
}
