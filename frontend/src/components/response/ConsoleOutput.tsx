import type { ConsoleLineDto } from "../../api/types";

/** What the request's scripts logged with `console.*`. */
export function ConsoleOutput({ lines }: { lines: ConsoleLineDto[] }) {
  if (lines.length === 0) return <p className="muted center">Scripts logged nothing.</p>;
  return (
    <ul className="console-output">
      {lines.map((line, i) => (
        <li key={i} className={line.level}>
          <pre>{line.text}</pre>
        </li>
      ))}
    </ul>
  );
}
