import type { HttpRequestDto } from "../../api/types";
import { syncPathParams, syncQueryFromUrl } from "../../lib/url";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "CONNECT", "TRACE"];

interface Props {
  draft: HttpRequestDto;
  dirty: boolean;
  sending: boolean;
  onEdit: (f: (d: HttpRequestDto) => HttpRequestDto) => void;
  onSend: () => void;
  onSave: () => void;
}

export function UrlBar({ draft, dirty, sending, onEdit, onSend, onSave }: Props) {
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
      <input
        aria-label="URL"
        className="url"
        value={draft.url}
        spellCheck={false}
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
      <button type="submit" className="primary" disabled={sending} title="Send (Enter, Ctrl/Cmd+Enter)">
        {sending ? "Sending…" : "Send"}
      </button>
      <button type="button" disabled={!dirty} onClick={onSave} title="Save (Ctrl/Cmd+S)">
        Save
      </button>
    </form>
  );
}
