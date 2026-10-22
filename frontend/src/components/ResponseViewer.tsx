import { useState } from "react";
import type { ScriptErrorDto, SendResultDto } from "../api/types";
import { CodeEditor } from "../editor/CodeEditor";
import { Banner } from "./Banner";
import { formatDuration, formatSize, prettyBody } from "../lib/format";
import { ConsoleOutput } from "./response/ConsoleOutput";
import { TestResults, testSummary } from "./response/TestResults";

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

type View = "body" | "headers" | "tests" | "console";

export function ResponseViewer({ result, sending }: { result?: SendResultDto; sending: boolean }) {
  const [view, setView] = useState<View>("body");
  if (sending) return <p className="muted center">Sending…</p>;
  if (!result) return <p className="muted center">Send the request to see the response.</p>;
  const response = result.response;
  const summary = testSummary(result.tests, result.asserts);
  const banners = (
    <>
      {result.scriptError && (
        <Banner kind="error" role="alert">
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
  const tabs = (
    <nav className="subtabs">
      {response && (
        <>
          <button className={view === "body" ? "active" : ""} onClick={() => setView("body")}>
            Body
          </button>
          <button className={view === "headers" ? "active" : ""} onClick={() => setView("headers")}>
            Headers<sup>{response.headers.length}</sup>
          </button>
        </>
      )}
      <button className={view === "tests" ? "active" : ""} onClick={() => setView("tests")}>
        Tests
        {summary.total > 0 && (
          <sup className={summary.passed === summary.total ? "ok" : "err"}>
            {summary.passed}/{summary.total}
          </sup>
        )}
      </button>
      <button className={view === "console" ? "active" : ""} onClick={() => setView("console")}>
        Console{result.console.length > 0 && <sup>{result.console.length}</sup>}
      </button>
    </nav>
  );
  const extra =
    view === "tests" ? (
      <TestResults tests={result.tests} asserts={result.asserts} />
    ) : view === "console" ? (
      <ConsoleOutput lines={result.console} />
    ) : null;
  if (!response) {
    return (
      <div className="response">
        <div className="response-meta">
          <span className="muted">No response</span>
          <span className="spacer" />
          {tabs}
        </div>
        {banners}
        {extra ?? <ConsoleOutput lines={result.console} />}
      </div>
    );
  }
  const body = prettyBody(response);
  return (
    <div className="response">
      <div className="response-meta">
        <span className={`status ${statusClass(response.status)}`}>{response.status}</span>
        <span title="Time">{formatDuration(response.elapsedMs)}</span>
        <span title={response.sizeComplete ? "Size (decoded)" : "Bytes read before the 10 MB limit"}>
          {response.sizeComplete ? formatSize(response.size) : `≥ ${formatSize(response.size)}`}
        </span>
        <span className="spacer" />
        {tabs}
      </div>
      {banners}
      {response.truncated && (
        <Banner kind="warn" role="status">
          {response.sizeComplete
            ? "Showing the first 10 MB of the body."
            : "Stopped reading after 10 MB; the full size is unknown. Showing the first 10 MB."}
        </Banner>
      )}
      {extra ?? (view === "body" ? (
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
          <tbody>
            {response.headers.map(([k, v], i) => (
              <tr key={i}>
                <th>{k}</th>
                <td>{v}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ))}
    </div>
  );
}
