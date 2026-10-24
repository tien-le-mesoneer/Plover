import type { RunEventDto } from "../../api/types";
import { formatDuration, formatSize } from "../../lib/format";
import { duplicateNames, type RunFilterValue, type RunListEntry, matchesFilter } from "../../lib/runRows";

const MARK = { pass: "✓", fail: "✕", skip: "⊘", running: "…" } as const;
const MARK_NAME = { pass: "Passed", fail: "Failed", skip: "Skipped", running: "Running" } as const;
const CHIP_MAX = 40;

function chipText(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

/** Passed/failed counts of one iteration. */
function iterationCounts(events: RunEventDto[], index: number): string {
  let passed = 0;
  let failed = 0;
  for (const e of events) {
    if (e.type === "passed" && e.iteration === index) passed++;
    if (e.type === "failed" && e.iteration === index) failed++;
  }
  return `${passed} passed, ${failed} failed`;
}

/**
 * The run's results in run order. A row's folder shows only when another row has the same
 * request name. Rows are focusable; Enter or a click opens the result.
 */
export function RunList({
  entries,
  events,
  filter,
  name,
  selected,
  onSelect,
}: {
  entries: RunListEntry[];
  events: RunEventDto[];
  filter: RunFilterValue;
  name: string;
  selected?: number;
  onSelect: (index: number) => void;
}) {
  const rows = entries.flatMap((e) => (e.kind === "row" ? [e] : []));
  const dup = duplicateNames(rows);
  const filtering = filter !== "all" || name.trim() !== "";
  return (
    <table className="run-list" aria-label="Run results">
      <thead className="sr-only">
        <tr>
          <th scope="col">Result</th>
          <th scope="col">Method</th>
          <th scope="col">Name</th>
          <th scope="col">Status · time</th>
        </tr>
      </thead>
      <tbody>
        {entries.map((e, i) => {
          if (e.kind === "iteration") {
            if (filtering) return null;
            return (
              <tr key={`it-${i}`} className="iteration-row">
                <th scope="rowgroup" colSpan={4}>
                  <span className="iteration-title">
                    Iteration {e.index + 1} of {e.total}
                  </span>
                  <span className="muted"> · {iterationCounts(events, e.index)}</span>
                  {Object.entries(e.data).map(([k, v]) => {
                    const text = chipText(v);
                    const cut = text.length > CHIP_MAX;
                    return (
                      <span key={k} className="data-chip" title={cut ? `${k}=${text}` : undefined}>
                        {k}={cut ? `${text.slice(0, CHIP_MAX)}…` : text}
                      </span>
                    );
                  })}
                </th>
              </tr>
            );
          }
          if (!matchesFilter(e, filter, name)) return null;
          const selectable = e.state !== "running";
          const open = () => selectable && onSelect(e.index);
          return (
            <tr
              key={`row-${i}`}
              className={`run-row ${selectable ? "selectable" : ""} state-${e.state}`}
              tabIndex={selectable ? 0 : -1}
              aria-selected={selectable ? selected === e.index : undefined}
              onClick={open}
              onKeyDown={(k) => {
                if (k.key === "Enter" || k.key === " ") {
                  k.preventDefault();
                  open();
                }
              }}
            >
              <td className={`mark state-${e.state}`}>
                <span aria-hidden="true">{MARK[e.state]}</span>
                <span className="sr-only">{MARK_NAME[e.state]}</span>
              </td>
              <td className="run-method">
                <span className={`badge method-${e.method.toLowerCase()}`}>{e.method}</span>
              </td>
              <td className="run-name">
                <span>{e.name}</span>
                {dup.has(e.name) && <span className="run-folder muted">{e.folder || "(collection root)"}</span>}
              </td>
              <td className="num run-meta">
                {e.state === "skip"
                  ? "skip"
                  : [e.status, e.ms != null ? formatDuration(e.ms) : null, e.size != null ? formatSize(e.size) : null]
                      .filter((x) => x != null && x !== "")
                      .join(" · ")}
                {e.checks && <span className="muted"> · {e.checks}</span>}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
