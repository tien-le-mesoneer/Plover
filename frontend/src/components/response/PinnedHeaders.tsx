import { CopyIcon } from "../icons";

/** Headers people send to backend teams to find a request in the logs (Claude's call C7). */
export const PINNED_HEADERS = ["x-request-id", "x-correlation-id", "traceparent", "x-trace-id", "location"];

/** The pinned headers the response has, in `PINNED_HEADERS` order, each with a copy button. */
export function PinnedHeaders({ headers, onCopy }: { headers: [string, string][]; onCopy: (text: string) => void }) {
  const present = PINNED_HEADERS.flatMap((name) => {
    const found = headers.find(([k]) => k.toLowerCase() === name);
    return found ? [[name, found[1]] as const] : [];
  });
  if (present.length === 0) return null;
  return (
    <ul className="pinned-headers" aria-label="Pinned headers">
      {present.map(([name, value]) => (
        <li key={name}>
          <span className="muted">{name}</span>
          <code title={value}>{value}</code>
          <button type="button" className="icon" aria-label={`Copy ${name}`} title={`Copy ${name}`} onClick={() => onCopy(value)}>
            <CopyIcon />
          </button>
        </li>
      ))}
    </ul>
  );
}
