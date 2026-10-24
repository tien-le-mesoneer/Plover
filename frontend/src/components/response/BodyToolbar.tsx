import type { KeyboardEvent } from "react";

export type BodyMode = "pretty" | "raw" | "preview";

const MODES: { mode: BodyMode; label: string }[] = [
  { mode: "pretty", label: "Pretty" },
  { mode: "raw", label: "Raw" },
  { mode: "preview", label: "Preview" },
];

/** Pretty / Raw / Preview, and the JSONPath filter (JSON bodies only). */
export function BodyToolbar({
  mode,
  onMode,
  filter,
  onFilter,
  filterError,
  isJson,
  canPreview,
  canPretty,
  jsonMismatch,
}: {
  mode: BodyMode;
  onMode: (m: BodyMode) => void;
  filter: string;
  onFilter: (f: string) => void;
  filterError?: string;
  isJson: boolean;
  canPreview: boolean;
  canPretty: boolean;
  /** The content type says JSON but the body does not parse (an HTML error page, say). */
  jsonMismatch?: boolean;
}) {
  const enabled = (m: BodyMode) => (m === "preview" ? canPreview : m === "pretty" ? canPretty : true);
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const list = MODES.filter((m) => enabled(m.mode));
    const at = list.findIndex((m) => m.mode === mode);
    const next = list[(at + (e.key === "ArrowRight" ? 1 : list.length - 1)) % list.length];
    if (!next) return;
    e.preventDefault();
    onMode(next.mode);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-mode="${next.mode}"]`)?.focus();
  };
  return (
    <div className="body-toolbar">
      <div className="segmented" role="radiogroup" aria-label="Body view" onKeyDown={onKeyDown}>
        {MODES.map((m) => (
          <button
            key={m.mode}
            type="button"
            role="radio"
            data-mode={m.mode}
            aria-checked={mode === m.mode}
            tabIndex={mode === m.mode ? 0 : -1}
            disabled={!enabled(m.mode)}
            title={
              enabled(m.mode)
                ? undefined
                : m.mode === "preview"
                  ? "Preview shows HTML, images and PDFs"
                  : "Body is not valid JSON"
            }
            onClick={() => onMode(m.mode)}
          >
            {m.label}
          </button>
        ))}
      </div>
      <input
        className="jsonpath"
        aria-label="Filter with JSONPath"
        placeholder="$.path.to[*].field"
        spellCheck={false}
        disabled={!isJson}
        title={isJson ? "JSONPath: $.key, ['key'], [0], [-1], [*], .*, ..key" : "Body is not valid JSON"}
        value={filter}
        onChange={(e) => onFilter(e.target.value)}
        aria-invalid={filterError ? true : undefined}
        aria-describedby={filterError ? "jsonpath-error" : undefined}
      />
      {jsonMismatch && <span className="muted">Body is not valid JSON</span>}
      {filterError && (
        <span id="jsonpath-error" className="field-error">
          {filterError}
        </span>
      )}
    </div>
  );
}
