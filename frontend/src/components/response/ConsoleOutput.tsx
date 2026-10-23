import type { ConsoleLineDto, ScriptedRequestDto } from "../../api/types";

/** What the request's scripts logged with `console.*` and sent with `bru.sendRequest`/`axios`. */
export function ConsoleOutput({
  lines,
  requests = [],
}: {
  lines: ConsoleLineDto[];
  requests?: ScriptedRequestDto[];
}) {
  if (lines.length === 0 && requests.length === 0) {
    return <p className="muted center">Scripts logged nothing.</p>;
  }
  return (
    <ul className="console-output">
      {lines.map((line, i) => (
        <li key={i} className={line.level}>
          <pre>{line.text}</pre>
        </li>
      ))}
      {requests.map((r, i) => (
        <li key={`r${i}`} className={r.status === null ? "error scripted" : "scripted"}>
          <pre>
            ↳ {r.source} {r.method} {r.url} → {r.status ?? `error: ${r.error ?? "no response"}`} ({r.durationMs} ms)
          </pre>
        </li>
      ))}
    </ul>
  );
}
