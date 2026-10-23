import type { ResponseDto } from "../api/types";

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

export function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

export type BodyLanguage = "json" | "xml" | "html" | "text" | "binary";

export function header(r: ResponseDto, name: string): string | undefined {
  const lower = name.toLowerCase();
  return r.headers.find(([k]) => k.toLowerCase() === lower)?.[1];
}

/** How to show a response body: language from content type, JSON pretty-printed. */
export function prettyBody(r: ResponseDto): { language: BodyLanguage; text: string } {
  if (r.bodyText === null) {
    return { language: "binary", text: `Binary body (${formatSize(r.size)}) — not shown.` };
  }
  const type = (header(r, "content-type") ?? "").toLowerCase();
  if (type.includes("json")) {
    try {
      return { language: "json", text: JSON.stringify(JSON.parse(r.bodyText), null, 2) };
    } catch {
      return { language: "json", text: r.bodyText };
    }
  }
  if (type.includes("xml")) return { language: "xml", text: r.bodyText };
  if (type.includes("html")) return { language: "html", text: r.bodyText };
  return { language: "text", text: r.bodyText };
}

const REASONS: Record<number, string> = {
  100: "Continue", 101: "Switching Protocols", 200: "OK", 201: "Created", 202: "Accepted",
  203: "Non-Authoritative Information", 204: "No Content", 205: "Reset Content", 206: "Partial Content",
  300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other", 304: "Not Modified",
  307: "Temporary Redirect", 308: "Permanent Redirect", 400: "Bad Request", 401: "Unauthorized",
  402: "Payment Required", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
  406: "Not Acceptable", 408: "Request Timeout", 409: "Conflict", 410: "Gone", 411: "Length Required",
  412: "Precondition Failed", 413: "Content Too Large", 414: "URI Too Long", 415: "Unsupported Media Type",
  416: "Range Not Satisfiable", 418: "I'm a teapot", 422: "Unprocessable Content", 425: "Too Early",
  426: "Upgrade Required", 428: "Precondition Required", 429: "Too Many Requests",
  431: "Request Header Fields Too Large", 451: "Unavailable For Legal Reasons", 500: "Internal Server Error",
  501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout",
  505: "HTTP Version Not Supported",
};

/** "200 OK"; just the code when the reason phrase is not a standard one. */
export function statusLine(status: number): string {
  const reason = REASONS[status];
  return reason ? `${status} ${reason}` : String(status);
}
