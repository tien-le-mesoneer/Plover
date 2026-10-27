import type { MouseEventHandler } from "react";
import { copyShortcut, hoverLines } from "../../lib/varText";
import type { VarInfo } from "../../lib/varTokens";
import { CopyIcon } from "../icons";

interface Props {
  id: string;
  name: string;
  info: VarInfo;
  env: string | null;
  /** The token's box on screen; the card sits under it. */
  anchor: { left: number; bottom: number };
  /** Copies the value; absent for names no scope defines yet. */
  onCopy?: () => void;
  /** The last copy's note. */
  note?: string | null;
  onMouseEnter?: MouseEventHandler<HTMLDivElement>;
  onMouseLeave?: MouseEventHandler<HTMLDivElement>;
}

/**
 * A variable's value and where it comes from. A small non-modal dialog (it holds a
 * button), linked to its field by aria-describedby; the pointer can move onto it.
 */
export function VarHover({ id, name, info, env, anchor, onCopy, note, onMouseEnter, onMouseLeave }: Props) {
  return (
    <div
      id={id}
      role="dialog"
      aria-label={`Variable ${name}`}
      className={`var-hover var-hover-${info.kind}`}
      style={{ left: anchor.left, top: anchor.bottom + 4 }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      // Clicking the card must not blur the field (which would close it).
      onMouseDown={(e) => e.preventDefault()}
    >
      <div className="var-hover-head">
        <span className="var-hover-name">{`{{${name}}}`}</span>
        {onCopy && (
          <button type="button" className="icon" aria-label="Copy value" title={`Copy value (${copyShortcut()})`} onClick={onCopy}>
            <CopyIcon />
          </button>
        )}
      </div>
      {hoverLines(info, env).map((line, i) => (
        <div key={i} className={line.value !== undefined ? "var-hover-value" : "var-hover-note"}>
          {line.value !== undefined && <code className={line.masked ? "muted" : ""}>{line.value}</code>}
          {line.note && <span className="muted"> {line.note}</span>}
        </div>
      ))}
      {onCopy && <div className="var-hover-note muted">{note ?? `${copyShortcut()} copies the value`}</div>}
    </div>
  );
}
