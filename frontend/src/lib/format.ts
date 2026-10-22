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
