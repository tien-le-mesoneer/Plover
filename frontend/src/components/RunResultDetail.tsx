import type { BodyDetailDto, RunResultDetailDto } from "../api/types";
import { formatSize } from "../lib/format";
import { useRuns } from "../store/runs";
import { useTabs } from "../store/tabs";
import { CloseIcon, ExternalIcon } from "./icons";
import { ResponseViewer } from "./ResponseViewer";

/** What the run kept of a response body, when it is not the whole body. */
function bodyNote(body: BodyDetailDto | null): string | undefined {
  if (!body) return undefined;
  if (body.kind === "omitted") return `Body not kept (${body.omitted ?? "unknown reason"})`;
  if (body.truncated) return `Showing the first 1 MiB of ${formatSize(body.size)}`;
  return undefined;
}

/** Why a result failed, shown first: the run's error, else the first failed check. */
function failureReason(d: RunResultDetailDto): string | null {
  if (d.status !== "fail" && d.status !== "error") return null;
  if (d.error) return d.error;
  const test = d.result.tests.find((t) => !t.passed);
  if (test) return `test "${test.name}"${test.error ? `: ${test.error}` : ""}`;
  const a = d.result.asserts.find((x) => !x.passed);
  if (a) return `assert ${a.lhs} ${a.operator} ${a.rhs}${a.error ? `: ${a.error}` : ""}`;
  return d.result.scriptError?.message ?? null;
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
        {data && (
          <button
            type="button"
            className="icon"
            title="Open request"
            aria-label="Open request"
            onClick={() => void useTabs.getState().open(run.collectionId, data.item).catch(() => undefined)}
          >
            <ExternalIcon />
          </button>
        )}
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
      {data && <OutcomeDetail data={data} collectionId={run.collectionId} />}
    </section>
  );
}

/**
 * A stored result, read only: why it failed (first), then the request as sent and the
 * response. Used by run results and the History tab.
 */
export function OutcomeDetail({ data, collectionId }: { data: RunResultDetailDto; collectionId: string }) {
  if (data.status === "skipped") return <p className="muted">Skipped: {data.skipReason}</p>;
  const reason = failureReason(data);
  return (
    <>
      {reason && (
        <p className="failure-reason" role="alert">
          <span aria-hidden="true">✕ </span>
          {reason}
        </p>
      )}
      <ResponseViewer
        result={data.result}
        sending={false}
        request={data.request}
        bodyNote={bodyNote(data.responseBody)}
        name={data.name}
        collectionId={collectionId}
      />
    </>
  );
}
