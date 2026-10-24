import { useEffect, useRef, useState } from "react";
import type { BodyDetailDto, RequestDetailDto, ScriptErrorDto, SendResultDto } from "../api/types";
import { CodeEditor } from "../editor/CodeEditor";
import { Banner } from "./Banner";
import { formatDuration, formatSize, prettyBody, statusLine } from "../lib/format";
import { CheckIcon, CopyIcon, CrossIcon } from "./icons";
import { ConsoleOutput } from "./response/ConsoleOutput";
import { TestResults, testSummary } from "./response/TestResults";
import { SubTabs, type SubTab } from "./SubTabs";

function statusClass(status: number): string {
  if (status >= 500) return "status-5xx";
  if (status >= 400) return "status-4xx";
  if (status >= 300) return "status-3xx";
  return "status-2xx";
}

export function scriptErrorText(e: ScriptErrorDto): string {
  const where = [e.scope, e.line ? `line ${e.line}` : null].filter(Boolean).join(", ");
  const what = e.name ? `${e.name}: ${e.message}` : e.message;
  return `${e.phase} script error${where ? ` (${where})` : ""}: ${what}`;
}

type View = "body" | "headers" | "request" | "tests" | "console";

/** Whether the request as a whole passed: script errors and failed checks win over the HTTP status. */
function outcome(result: SendResultDto): { ok: boolean; text: string } | null {
  if (result.scriptError) return { ok: false, text: "Script error" };
  const { passed, total } = testSummary(result.tests, result.asserts);
  if (total === 0) return null;
  if (passed === total) return { ok: true, text: `${total} ${total === 1 ? "check" : "checks"} passed` };
  return { ok: false, text: `${total - passed} of ${total} checks failed` };
}

interface Props {
  result?: SendResultDto;
  sending: boolean;
  /** Shows the request's scripts (used by the script-error banner). */
  onOpenScript?: () => void;
  /** The request as sent (a run result): adds a Request tab, and Body becomes Response. */
  request?: RequestDetailDto | null;
  /** Replaces the truncation notice, e.g. for a body a run kept only in part. */
  bodyNote?: string;
}

/** A stored request body: text, form or multipart fields, or a note. */
export function BodyDetail({ body }: { body: BodyDetailDto }) {
  switch (body.kind) {
    case "none":
      return <p className="muted">No body</p>;
    case "text":
      return (
        <>
          <pre className="body-text">{body.text}</pre>
          {body.truncated && (
            <p className="muted">
              Showing the first {formatSize((body.text ?? "").length)} of {formatSize(body.size)}
            </p>
          )}
        </>
      );
    case "base64":
      return <p className="muted">Binary body, {body.size} bytes</p>;
    case "omitted":
      return <p className="muted">Body not kept ({body.omitted})</p>;
    default: {
      const rows: [string, string][] =
        body.kind === "form"
          ? body.fields
          : body.parts.map((p) => [p.name, p.fileName !== null ? `file ${p.fileName}` : (p.value ?? "")]);
      return <KeyValueRows rows={rows} label="Body fields" />;
    }
  }
}

function KeyValueRows({ rows, label }: { rows: [string, string][]; label: string }) {
  return (
    <table className="headers" aria-label={label}>
      <thead>
        <tr>
          <th scope="col">Name</th>
          <th scope="col">Value</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([k, v], i) => (
          <tr key={i}>
            <th scope="row">{k}</th>
            <td>{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function RequestView({ request }: { request: RequestDetailDto }) {
  return (
    <div className="request-detail">
      <p>
        <code>
          <span className={`badge method-${request.method.toLowerCase()}`}>{request.method}</span> {request.url}
        </code>
      </p>
      {request.headers.length > 0 && <KeyValueRows rows={request.headers} label="Request headers" />}
      <BodyDetail body={request.body} />
    </div>
  );
}

export function ResponseViewer({ result, sending, onOpenScript, request, bodyNote }: Props) {
  const failedChecks = result ? testSummary(result.tests, result.asserts) : { passed: 0, total: 0 };
  const hasFailures = failedChecks.passed < failedChecks.total;
  const [view, setView] = useState<View>(hasFailures ? "tests" : "body");
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [shown, setShown] = useState(result);

  // A new result with failing checks opens the Tests tab, so the failure is not
  // hidden behind a green 200 (state adjusted during render, not in an effect).
  if (result !== shown) {
    setShown(result);
    if (hasFailures) setView("tests");
  }
  useEffect(() => () => clearTimeout(timer.current), []);

  if (sending) {
    return (
      <p className="muted center" aria-busy="true">
        Sending…
      </p>
    );
  }
  if (!result) return <p className="muted center">Send the request to see the response.</p>;
  const response = result.response;
  const summary = testSummary(result.tests, result.asserts);
  const verdict = outcome(result);
  const banners = (
    <>
      {result.scriptError && (
        <Banner
          kind="error"
          role="alert"
          actions={onOpenScript && <button onClick={onOpenScript}>Open script</button>}
        >
          {scriptErrorText(result.scriptError)}
        </Banner>
      )}
      {result.skipped && (
        <Banner kind="warn" role="status">
          Not sent: {result.skipped}
        </Banner>
      )}
    </>
  );
  const consoleCount = result.console.length + result.scriptedRequests.length;
  const tabList: SubTab<View>[] = [
    ...(response
      ? [
          { id: "body" as const, label: request ? "Response" : "Body", name: "Response body" },
          {
            id: "headers" as const,
            label: (
              <>
                Headers<sup aria-hidden="true">{response.headers.length}</sup>
              </>
            ),
            name: `Response headers, ${response.headers.length}`,
          },
        ]
      : []),
    ...(request
      ? [{ id: "request" as const, label: "Request", name: "Request as sent" }]
      : []),
    {
      id: "tests",
      label: (
        <>
          Tests
          {summary.total > 0 && (
            <sup aria-hidden="true" className={summary.passed === summary.total ? "ok" : "err"}>
              {summary.passed}/{summary.total}
            </sup>
          )}
        </>
      ),
      name: summary.total > 0 ? `Tests, ${summary.passed} of ${summary.total} passed` : "Tests",
    },
    {
      id: "console",
      label: (
        <>
          Console{consoleCount > 0 && <sup aria-hidden="true">{consoleCount}</sup>}
        </>
      ),
      name: consoleCount > 0 ? `Console, ${consoleCount} entries` : "Console",
    },
  ];
  const current: View = !response && (view === "body" || view === "headers") ? "console" : view;
  const tabs = <SubTabs tabs={tabList} value={current} onChange={setView} label="Response sections" />;
  const chip = verdict && (
    <span className={`outcome ${verdict.ok ? "ok" : "err"}`}>
      {verdict.ok ? <CheckIcon /> : <CrossIcon />}
      {verdict.text}
    </span>
  );
  const extra =
    current === "tests" ? (
      <TestResults tests={result.tests} asserts={result.asserts} />
    ) : current === "request" && request ? (
      <RequestView request={request} />
    ) : current === "console" ? (
      <ConsoleOutput lines={result.console} requests={result.scriptedRequests} />
    ) : null;
  if (!response) {
    return (
      <div className="response">
        <div className="response-meta">
          {chip}
          <span className="muted">No response</span>
          <span className="spacer" />
        </div>
        {tabs}
        {banners}
        {extra}
      </div>
    );
  }
  const body = prettyBody(response);
  const copyText =
    current === "headers" ? response.headers.map(([k, v]) => `${k}: ${v}`).join("\n") : body.text;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(copyText);
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };
  const canCopy = (current === "body" && body.language !== "binary") || current === "headers";
  return (
    <div className="response">
      <div className="response-meta">
        {chip}
        <span className={`status ${statusClass(response.status)}`}>{statusLine(response.status)}</span>
        <span className="num" title="Time">
          {formatDuration(response.elapsedMs)}
        </span>
        <span className="num" title={response.sizeComplete ? "Size (decoded)" : "Bytes read before the 10 MB limit"}>
          {response.sizeComplete ? formatSize(response.size) : `≥ ${formatSize(response.size)}`}
        </span>
        <span className="spacer" />
        {canCopy && (
          <button
            type="button"
            className="copy"
            aria-label={current === "headers" ? "Copy headers" : "Copy body"}
            onClick={() => void copy()}
          >
            <CopyIcon />
            <span aria-live="polite">{copied ? "Copied" : "Copy"}</span>
          </button>
        )}
      </div>
      {tabs}
      {banners}
      {bodyNote ? (
        <Banner kind="warn" role="status">
          {bodyNote}
        </Banner>
      ) : response.truncated && (
        <Banner kind="warn" role="status">
          {response.sizeComplete
            ? "Showing the first 10 MB of the body."
            : "Stopped reading after 10 MB; the full size is unknown. Showing the first 10 MB."}
        </Banner>
      )}
      {extra ??
        (current === "body" ? (
          body.language === "binary" ? (
            <p className="muted center">{body.text}</p>
          ) : (
            <div className="editor-box">
              <CodeEditor
                value={body.text}
                language={body.language === "text" ? "plaintext" : body.language}
                readOnly
              />
            </div>
          )
        ) : (
          <table className="headers">
            <thead>
              <tr>
                <th scope="col">Header</th>
                <th scope="col">Value</th>
              </tr>
            </thead>
            <tbody>
              {response.headers.map(([k, v], i) => (
                <tr key={i}>
                  <th scope="row">{k}</th>
                  <td>{v}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ))}
    </div>
  );
}
