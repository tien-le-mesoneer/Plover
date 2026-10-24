import { hoverLines } from "../../lib/varText";
import type { VarInfo } from "../../lib/varTokens";

interface Props {
  id: string;
  name: string;
  info: VarInfo;
  env: string | null;
  /** The token's box on screen; the card sits under it. */
  anchor: { left: number; bottom: number };
}

/** A variable's value and where it comes from (role tooltip, linked by aria-describedby). */
export function VarHover({ id, name, info, env, anchor }: Props) {
  return (
    <div id={id} role="tooltip" className={`var-hover var-hover-${info.kind}`} style={{ left: anchor.left, top: anchor.bottom + 4 }}>
      <div className="var-hover-name">{`{{${name}}}`}</div>
      {hoverLines(info, env).map((line, i) => (
        <div key={i} className={line.value !== undefined ? "var-hover-value" : "var-hover-note"}>
          {line.value !== undefined && <code className={line.masked ? "muted" : ""}>{line.value}</code>}
          {line.note && <span className="muted"> {line.note}</span>}
        </div>
      ))}
    </div>
  );
}
