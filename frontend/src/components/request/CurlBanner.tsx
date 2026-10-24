import type { CurlNoticeDto, EnvironmentDto, HttpRequestDto, KeyValueDto } from "../../api/types";
import { Banner } from "../Banner";

/** Header names whose values "Move to secret" turns into secret variables. */
const SECRET_HEADERS: Record<string, string> = { authorization: "authorization", cookie: "cookie" };

/** The headers to move, each with the variable it gets (`authorization`, `authorization2`, …). */
export function secretMoves(draft: HttpRequestDto, env: EnvironmentDto | undefined): { index: number; variable: string }[] {
  const taken = new Set([...(env?.vars.map((v) => v.name) ?? []), ...(env?.secretNames ?? [])]);
  const moves: { index: number; variable: string }[] = [];
  draft.headers.forEach((h: KeyValueDto, index) => {
    const base = SECRET_HEADERS[h.name.toLowerCase()];
    if (!base || !h.value || /^\{\{[^}]+\}\}$/.test(h.value.trim())) return;
    let variable = base;
    for (let n = 2; taken.has(variable); n++) variable = `${base}${n}`;
    taken.add(variable);
    moves.push({ index, variable });
  });
  return moves;
}

interface Props {
  notices: CurlNoticeDto[];
  /** Whether the draft has an `Authorization` or `Cookie` value to move. */
  canMove: boolean;
  /** The selected environment; moving needs one. */
  env: string | null;
  moving: boolean;
  onUndo: () => void;
  onMoveToSecret: () => void;
  onDismiss: () => void;
}

/** "Filled from cURL", the options that were not imported, Undo and Move to secret. */
export function CurlBanner({ notices, canMove, env, moving, onUndo, onMoveToSecret, onDismiss }: Props) {
  return (
    <Banner
      kind="warn"
      role="status"
      actions={
        <>
          {canMove && (
            <button
              type="button"
              disabled={!env || moving}
              title={env ? `Store the values as secrets of ${env}` : "Select an environment first"}
              onClick={onMoveToSecret}
            >
              Move to secret
            </button>
          )}
          <button type="button" onClick={onUndo}>
            Undo
          </button>
          <button type="button" onClick={onDismiss}>
            Dismiss
          </button>
        </>
      }
    >
      <strong>Filled from cURL</strong>
      {notices.length > 0 && (
        <>
          {" "}
          <span>Not imported:</span>
          <ul className="curl-notices">
            {notices.map((n, i) => (
              <li key={i}>
                <code>{n.option}</code> ({n.reason})
              </li>
            ))}
          </ul>
        </>
      )}
    </Banner>
  );
}
