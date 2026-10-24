import type { SendResultDto } from "../../api/types";
import { formatDuration, formatSize } from "../../lib/format";
import { statusGroup, statusText } from "../../lib/status";
import { CheckIcon, CopyIcon, CrossIcon, DownloadIcon } from "../icons";
import { testSummary } from "./TestResults";

/** The checks summary: script errors and failed checks win over the HTTP status. */
function outcome(result: SendResultDto): { ok: boolean; text: string; name: string } | null {
  if (result.scriptError) return { ok: false, text: "Script error", name: "Script error" };
  const { passed, total } = testSummary(result.tests, result.asserts);
  if (total === 0) return null;
  return { ok: passed === total, text: `${passed}/${total}`, name: `Checks: ${passed} of ${total} passed` };
}

/**
 * Status (colour by group, always with its text), time and size, the checks summary, then
 * small icon buttons for copying the body and saving it to a file.
 */
export function StatusLine({
  result,
  onCopyBody,
  onSaveBody,
  note,
}: {
  result: SendResultDto;
  /** Absent when there is nothing to copy (no response, binary body). */
  onCopyBody?: () => void;
  onSaveBody?: () => void;
  note: string | null;
}) {
  const response = result.response;
  const verdict = outcome(result);
  return (
    <div className="response-meta">
      {response ? (
        <span className={`status status-${statusGroup(response.status)}`}>{statusText(response.status)}</span>
      ) : (
        <span className="muted">No response</span>
      )}
      {response && (
        <span className="num" title={response.sizeComplete ? "Time · size (decoded)" : "Time · bytes read before the 10 MB limit"}>
          {formatDuration(response.elapsedMs)} · {response.sizeComplete ? formatSize(response.size) : `≥ ${formatSize(response.size)}`}
        </span>
      )}
      {verdict && (
        <span className={`outcome ${verdict.ok ? "ok" : "err"}`} role="img" aria-label={verdict.name} title={verdict.name}>
          {verdict.ok ? <CheckIcon /> : <CrossIcon />}
          {verdict.text}
        </span>
      )}
      <span className="spacer" />
      <span className="copy-live" aria-live="polite">
        {note}
      </span>
      {onCopyBody && (
        <button type="button" className="icon" aria-label="Copy body" title="Copy body" onClick={onCopyBody}>
          <CopyIcon />
        </button>
      )}
      {onSaveBody && (
        <button type="button" className="icon" aria-label="Save body to file" title="Save body to file" onClick={onSaveBody}>
          <DownloadIcon />
        </button>
      )}
    </div>
  );
}
