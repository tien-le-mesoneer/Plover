import type { ResponseDto } from "../api/types";
import { header } from "./format";

/** File extension for a response's content type. */
export function extensionFor(contentType: string | undefined): string {
  const type = (contentType ?? "").toLowerCase();
  if (type.includes("json")) return ".json";
  if (type.includes("html")) return ".html";
  if (type.includes("xml")) return ".xml";
  if (type.startsWith("text/")) return ".txt";
  return ".bin";
}

/** A safe file name from a request name plus the body's extension. */
export function downloadName(name: string, body: ResponseDto): string {
  const base = name.replace(/[^\w.-]+/g, "-").replace(/^-+|-+$/g, "") || "response";
  return base + extensionFor(header(body, "content-type"));
}

function bodyBytes(body: ResponseDto): BlobPart {
  if (body.bodyText !== null) return body.bodyText;
  const raw = atob(body.bodyBase64 ?? "");
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

/** Saves the whole response body through a temporary `<a download>`. */
export function downloadBody(name: string, body: ResponseDto): void {
  const type = header(body, "content-type") ?? "application/octet-stream";
  const url = URL.createObjectURL(new Blob([bodyBytes(body)], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = downloadName(name, body);
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
