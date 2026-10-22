import { useState } from "react";
import type { ResponseDto } from "../api/types";
import { CodeEditor } from "../editor/CodeEditor";
import { Banner } from "./Banner";
import { formatDuration, formatSize, prettyBody } from "../lib/format";

function statusClass(status: number): string {
  if (status >= 500) return "status-5xx";
  if (status >= 400) return "status-4xx";
  if (status >= 300) return "status-3xx";
  return "status-2xx";
}

export function ResponseViewer({ response, sending }: { response?: ResponseDto; sending: boolean }) {
  const [view, setView] = useState<"body" | "headers">("body");
  if (sending) return <p className="muted center">Sending…</p>;
  if (!response) return <p className="muted center">Send the request to see the response.</p>;
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
        <nav className="subtabs">
          <button className={view === "body" ? "active" : ""} onClick={() => setView("body")}>
            Body
          </button>
          <button className={view === "headers" ? "active" : ""} onClick={() => setView("headers")}>
            Headers<sup>{response.headers.length}</sup>
          </button>
        </nav>
      </div>
      {response.truncated && (
        <Banner kind="warn" role="status">
          {response.sizeComplete
            ? "Showing the first 10 MB of the body."
            : "Stopped reading after 10 MB; the full size is unknown. Showing the first 10 MB."}
        </Banner>
      )}
      {view === "body" ? (
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
      )}
    </div>
  );
}
