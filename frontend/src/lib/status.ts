import { statusLine } from "./format";

export type StatusGroup = "ok" | "info" | "warn" | "err";

/** 2xx ok, 1xx/3xx info, 4xx warn, 5xx and transport errors (no status) err. */
export function statusGroup(status: number | null): StatusGroup {
  if (status === null) return "err";
  if (status >= 500) return "err";
  if (status >= 400) return "warn";
  if (status >= 200 && status < 300) return "ok";
  return "info";
}

/** `404 Not Found`, or the transport error code (`ECONNREFUSED`) when there is no status. */
export function statusText(status: number | null, errorCode?: string | null): string {
  if (status === null) return errorCode || "No response";
  return statusLine(status);
}
