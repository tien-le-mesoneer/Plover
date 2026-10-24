import type { BodyDetailDto } from "../api/types";
import { formatSize } from "../lib/format";
import { useRuns } from "../store/runs";
import { CloseIcon } from "./icons";
import { ResponseViewer } from "./ResponseViewer";

/** What the run kept of a response body, when it is not the whole body. */
function bodyNote(body: BodyDetailDto | null): string | undefined {
  if (!body) return undefined;
  if (body.kind === "omitted") return `Body not kept (${body.omitted ?? "unknown reason"})`;
  if (body.truncated) return `Showing the first 1 MiB of ${formatSize(body.size)}`;
  return undefined;
}

/** The request and response of the selected result of a run (live or saved). */
export function RunResultDetail({ runId }: { runId: string }) {
  const run = useRuns((s) => s.runs[runId]);
  const detail = run?.detail;
  if (!run || !detail) return null;
  const close = () => useRuns.setState((s) => ({ runs: { ...s.runs, [runId]: { ...run, selected: undefined, detail: undefined } } }));
  const data = detail.data;
  return (
    <section className="run-detail" aria-label="Result detail">
      <header>
        <strong>{data?.name ?? `Result ${detail.index + 1}`}</strong>
        {data && <span className={`result-${data.status}`}>{data.status}</span>}
        <span className="spacer" />
        <button className="icon" title="Close result detail" aria-label="Close result detail" onClick={close}>
          <CloseIcon />
        </button>
      </header>
      {detail.loading && (
        <p className="muted center" aria-busy="true">
          Loading…
        </p>
      )}
      {detail.error && <p className="err">{detail.error}</p>}
      {data &&
        (data.status === "skipped" ? (
          <p className="muted">Skipped: {data.skipReason}</p>
        ) : (
          <>
            {data.error && data.status === "error" && <p className="err">{data.error}</p>}
            <ResponseViewer
              result={data.result}
              sending={false}
              request={data.request}
              bodyNote={bodyNote(data.responseBody)}
              name={data.name}
              collectionId={run.collectionId}
            />
          </>
        ))}
    </section>
  );
}
