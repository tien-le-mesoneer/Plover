import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { BodyDetailDto, RequestDetailDto, ScriptErrorDto, SendResultDto } from "../api/types";
import { CodeEditor } from "../editor/CodeEditor";
import type { monaco } from "../editor/monaco";
import { formatElapsed } from "../lib/elapsed";
import { Banner } from "./Banner";
import { foldTargets, renderSlice } from "../lib/body";
import { downloadBody } from "../lib/download";
import { formatSize, header, prettyBody } from "../lib/format";
import { evaluateJsonPath, filterText, type JsonPathResult } from "../lib/jsonpath";
import { jsonPathAt, type ValueAt } from "../lib/jsonPathAt";
import { ValueMenu } from "./response/ValueMenu";
import { BodyPreview, previewKind } from "./response/BodyPreview";
import { BodyToolbar, type BodyMode } from "./response/BodyToolbar";
import { ConsoleOutput } from "./response/ConsoleOutput";
import { HeadersTable } from "./response/HeadersTable";
import { PinnedHeaders } from "./response/PinnedHeaders";
import { StatusLine } from "./response/StatusLine";
import { useCopy } from "./response/useCopy";
import { TestResults, testSummary } from "./response/TestResults";
import { SubTabs, type SubTab } from "./SubTabs";

export function scriptErrorText(e: ScriptErrorDto): string {
  const where = [e.scope, e.line ? `line ${e.line}` : null].filter(Boolean).join(", ");
  const what = e.name ? `${e.name}: ${e.message}` : e.message;
  return `${e.phase} script error${where ? ` (${where})` : ""}: ${what}`;
}

type View = "body" | "headers" | "request" | "tests" | "console";

interface Props {
  result?: SendResultDto;
  sending: boolean;
  /** Shows the request's scripts (used by the script-error banner). */
  onOpenScript?: () => void;
  /** The request as sent (a run result): adds a Request tab, and Body becomes Response. */
  request?: RequestDetailDto | null;
  /** Replaces the truncation notice, e.g. for a body a run kept only in part. */
  bodyNote?: string;
  /** The request's name, for "Save body to file". */
  name?: string;
  /** For "Save as runtime variable" in the value menu. */
  collectionId?: string;
  /** The last send was cancelled after this long. */
  cancelledAfterMs?: number;
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

interface BodyView {
  language: ReturnType<typeof prettyBody>["language"];
  /** Pretty-printed text (JSON indented), or the binary note. */
  text: string;
  parsed: { ok: true; value: unknown } | { ok: false };
  jsonMismatch: boolean;
}

/** Parses a response body once per result: pretty text, the JSON value, and whether a JSON content type lies. */
function bodyView(response: SendResultDto["response"]): BodyView | null {
  if (!response) return null;
  const pretty = prettyBody(response);
  const jsonType = (header(response, "content-type") ?? "").toLowerCase().includes("json");
  let parsed: BodyView["parsed"] = { ok: false };
  const text = response.bodyText;
  if (text !== null && (jsonType || /^\s*[[{]/.test(text))) {
    try {
      parsed = { ok: true, value: JSON.parse(text) };
    } catch {
      parsed = { ok: false };
    }
  }
  return { language: pretty.language, text: pretty.text, parsed, jsonMismatch: jsonType && text !== null && !parsed.ok };
}

export function ResponseViewer({ result, sending, onOpenScript, request, bodyNote, name, collectionId, cancelledAfterMs }: Props) {
  const failedChecks = result ? testSummary(result.tests, result.asserts) : { passed: 0, total: 0 };
  const hasFailures = failedChecks.passed < failedChecks.total;
  const [view, setView] = useState<View>(hasFailures ? "tests" : "body");
  const [shown, setShown] = useState(result);
  const { note, copy } = useCopy();
  const [mode, setMode] = useState<BodyMode>("pretty");
  const [filter, setFilter] = useState("");
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [picked, setPicked] = useState<{ at: { x: number; y: number }; value: ValueAt } | null>(null);
  const bodyInfo = useMemo(() => bodyView(result?.response ?? null), [result]);

  // A new result with failing checks opens the Tests tab, so the failure is not
  // hidden behind a green 200 (state adjusted during render, not in an effect).
  if (result !== shown) {
    setShown(result);
    setShowAll(false);
    setPicked(null);
    if (hasFailures) setView("tests");
  }

  if (sending) {
    return (
      <p className="muted center" aria-busy="true">
        Sending…
      </p>
    );
  }
  if (!result && cancelledAfterMs !== undefined) {
    return (
      <Banner kind="info" role="status">
        Cancelled after {formatElapsed(cancelledAfterMs)}
      </Banner>
    );
  }
  if (!result) return <p className="muted center">Send the request to see the response.</p>;
  const response = result.response;
  const summary = testSummary(result.tests, result.asserts);
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
          { id: "body" as const, label: "Body", name: "Response body" },
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
    ...(request ? [{ id: "request" as const, label: "Request", name: "Request as sent" }] : []),
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
      label: <>Console{consoleCount > 0 && <sup aria-hidden="true">{consoleCount}</sup>}</>,
      name: consoleCount > 0 ? `Console, ${consoleCount} entries` : "Console",
    },
  ];
  const current: View = !response && (view === "body" || view === "headers") ? "console" : view;
  const tabs = <SubTabs tabs={tabList} value={current} onChange={setView} label="Response sections" />;
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
        <StatusLine result={result} note={note} />
        {tabs}
        {banners}
        {extra}
      </div>
    );
  }
  const body = bodyInfo!;
  const textBody = body.language !== "binary";
  const filterResult: JsonPathResult | null = filter.trim() && body.parsed.ok ? evaluateJsonPath(body.parsed.value, filter) : null;
  const filtered = filterResult?.ok ? filterText(filterResult) : null;
  const preview = previewKind(response);
  // A JSON content type with a body that is not JSON (a proxy's HTML error page) shows Raw.
  const shownMode: BodyMode =
    mode === "preview" && !preview ? "pretty" : mode === "pretty" && body.jsonMismatch ? "raw" : mode;
  const fullText = filtered ?? (shownMode === "raw" ? (response.bodyText ?? "") : body.text);
  // At most 1 MiB goes into the editor until "Show all"; Copy and Save use the whole body.
  const slice = showAll ? { shown: fullText, capped: false, total: fullText.length } : renderSlice(fullText);
  const shownText = slice.shown;
  const jsonShown = body.parsed.ok && (filtered !== null || shownMode === "pretty");
  const folds = jsonShown ? foldTargets(shownText).map((f) => f.startLine) : undefined;
  // The value menu works on the pretty (or filtered) JSON text that is shown.
  const pickable = jsonShown;
  const onPick = (offset: number, at: { x: number; y: number }) => {
    if (!pickable) return;
    const value = jsonPathAt(shownText, offset);
    if (value) setPicked({ at, value });
  };
  const closePicked = () => {
    setPicked(null);
    editor.current?.focus();
  };
  const onFind = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "f" || !editor.current) return;
    e.preventDefault();
    editor.current.focus();
    void editor.current.getAction("actions.find")?.run();
  };
  return (
    <div className="response" onKeyDown={onFind}>
      <StatusLine
        result={result}
        note={note}
        onCopyBody={textBody ? () => void copy(filtered ?? response.bodyText ?? "") : undefined}
        onSaveBody={() => downloadBody(name ?? "response", response)}
      />
      {tabs}
      {banners}
      {bodyNote ? (
        <Banner kind="warn" role="status">
          {bodyNote}
        </Banner>
      ) : (
        response.truncated && (
          <Banner kind="warn" role="status">
            {response.sizeComplete
              ? "Showing the first 10 MB of the body."
              : "Stopped reading after 10 MB; the full size is unknown. Showing the first 10 MB."}
          </Banner>
        )
      )}
      {extra ??
        (current === "body" ? (
          <>
            {(textBody || preview) && (
              <BodyToolbar
                mode={shownMode}
                onMode={setMode}
                filter={filter}
                onFilter={setFilter}
                filterError={filterResult && !filterResult.ok ? filterResult.error : undefined}
                isJson={body.parsed.ok}
                jsonMismatch={body.jsonMismatch}
                canPreview={preview !== null}
                canPretty={!body.jsonMismatch}
              />
            )}
            {slice.capped && shownMode !== "preview" && (
              <Banner kind="warn" role="status" actions={<button onClick={() => setShowAll(true)}>Show all</button>}>
                Showing the first {formatSize(slice.shown.length)} of {formatSize(slice.total)}. Copy and Save use the whole body.
              </Banner>
            )}
            {shownMode === "preview" && preview ? (
              <BodyPreview response={response} kind={preview} />
            ) : textBody ? (
              <div className="editor-box">
                <CodeEditor
                  value={shownText}
                  language={filtered !== null ? "json" : shownMode === "raw" ? "plaintext" : body.language === "text" ? "plaintext" : body.language}
                  readOnly
                  onReady={(e) => (editor.current = e)}
                  onPick={onPick}
                  foldLines={folds}
                />
              </div>
            ) : (
              <p className="muted center">{body.text}</p>
            )}
            <PinnedHeaders headers={response.headers} onCopy={(v) => void copy(v)} />
            {picked && (
              <ValueMenu
                at={picked.at}
                path={picked.value.path}
                value={picked.value.value}
                collectionId={collectionId ?? null}
                pathDisabled={filtered !== null ? "Paths are relative to the filtered result" : undefined}
                onCopy={(text) => void copy(text)}
                onSaved={(n) => void copy(`{{${n}}}`, `Saved {{${n}}}`)}
                onClose={closePicked}
              />
            )}
          </>
        ) : (
          <HeadersTable headers={response.headers} onCopy={(v) => void copy(v)} />
        ))}
    </div>
  );
}
