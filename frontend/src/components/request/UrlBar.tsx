import { useEffect, useState, type ReactNode } from "react";
import type { HttpRequestDto } from "../../api/types";
import { formatElapsed } from "../../lib/elapsed";
import { syncPathParams, syncQueryFromUrl } from "../../lib/url";
import { VarInput } from "./VarInput";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "CONNECT", "TRACE"];

interface Props {
  draft: HttpRequestDto;
  sending: boolean;
  /** When the running send started (`Date.now()`), for the Cancel timer. */
  sendStartedAt?: number;
  onEdit: (f: (d: HttpRequestDto) => HttpRequestDto) => void;
  onSend: () => void;
  /** Stops the running send (the Cancel button that replaces Send while sending). */
  onCancel?: () => void;
  /** A pasted `curl …` command, instead of pasting it as the URL. */
  onCurl?: (text: string) => void;
  /** Controls after Send (the `⋯` request menu). */
  actions?: ReactNode;
  /** The request tab: `{{variables}}` in the URL are underlined and explained. */
  tabKey?: string;
}

const CURL = /^\s*curl\s/i;

/** Method · URL · Send (the only coloured button on the screen) · request menu. Save is ⌘S or the menu. */
export function UrlBar({ draft, sending, sendStartedAt, onEdit, onSend, onCancel, onCurl, actions, tabKey }: Props) {
  const methods = METHODS.includes(draft.method) ? METHODS : [...METHODS, draft.method];
  return (
    <form
      className="urlbar"
      onSubmit={(e) => {
        e.preventDefault();
        onSend();
      }}
    >
      <select
        aria-label="Method"
        className={`method method-${draft.method.toLowerCase()}`}
        value={draft.method}
        onChange={(e) => onEdit((d) => ({ ...d, method: e.target.value }))}
      >
        {methods.map((m) => (
          <option key={m}>{m}</option>
        ))}
      </select>
      {tabKey ? (
        <VarInput
          tabKey={tabKey}
          wrapperClassName="url-wrap"
        aria-label="URL"
          className="url"
          value={draft.url}
          spellCheck={false}
          onPaste={(e) => {
            const text = e.clipboardData.getData("text");
            if (!onCurl || !CURL.test(text)) return;
            e.preventDefault();
            onCurl(text);
          }}
          onKeyDown={(e) => {
            // Cmd/Ctrl+Enter sends through the app-wide shortcut; stop the form's
            // implicit submission so it is not sent twice.
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) e.preventDefault();
          }}
          onChange={(e) => {
            const url = e.target.value;
            onEdit((d) => ({
              ...d,
              url,
              query: syncQueryFromUrl(url, d.query),
              pathParams: syncPathParams(url, d.pathParams),
            }));
          }}
        />
      ) : (
        <input
        aria-label="URL"
        className="url"
        value={draft.url}
        spellCheck={false}
        onPaste={(e) => {
          const text = e.clipboardData.getData("text");
          if (!onCurl || !CURL.test(text)) return;
          e.preventDefault();
          onCurl(text);
        }}
        onKeyDown={(e) => {
          // Cmd/Ctrl+Enter sends through the app-wide shortcut; stop the form's
          // implicit submission so it is not sent twice.
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) e.preventDefault();
        }}
        onChange={(e) => {
          const url = e.target.value;
          onEdit((d) => ({
            ...d,
            url,
            query: syncQueryFromUrl(url, d.query),
            pathParams: syncPathParams(url, d.pathParams),
          }));
        }}
        />
      )}
      {sending && onCancel ? (
        <CancelButton startedAt={sendStartedAt} onCancel={onCancel} />
      ) : (
        <button type="submit" className="send" disabled={sending} title="Send (Enter, Ctrl/Cmd+Enter)">
          {sending ? "Sending…" : "Send"}
        </button>
      )}
      {actions}
    </form>
  );
}

/** Neutral outline button with the running time; Esc does the same (see App). */
function CancelButton({ startedAt, onCancel }: { startedAt?: number; onCancel: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  const [start] = useState(() => startedAt ?? Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(timer);
  }, []);
  return (
    <button type="button" className="cancel-send" aria-label="Cancel request" title="Cancel (Esc)" onClick={onCancel}>
      Cancel · <span aria-live="off">{formatElapsed(Math.max(0, now - (startedAt ?? start)))}</span>
    </button>
  );
}
