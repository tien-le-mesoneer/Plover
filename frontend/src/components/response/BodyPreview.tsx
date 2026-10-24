import { useEffect, useRef } from "react";
import type { ResponseDto } from "../../api/types";
import { header } from "../../lib/format";

export type PreviewKind = "html" | "image" | "pdf" | null;

/** What Preview can show for this response, or `null` (with the button disabled). */
export function previewKind(r: ResponseDto): PreviewKind {
  const type = (header(r, "content-type") ?? "").toLowerCase();
  if (type.includes("text/html")) return r.bodyText !== null ? "html" : null;
  if (type.startsWith("image/")) return "image";
  if (type.includes("application/pdf")) return "pdf";
  return null;
}

function blobOf(r: ResponseDto): Blob {
  const type = header(r, "content-type") ?? "application/octet-stream";
  if (r.bodyText !== null) return new Blob([r.bodyText], { type });
  const raw = atob(r.bodyBase64 ?? "");
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return new Blob([bytes], { type });
}

/**
 * HTML in a sandboxed frame (no scripts, no same-origin), images and PDFs from an object
 * URL that is revoked when the preview goes away.
 */
export function BodyPreview({ response, kind }: { response: ResponseDto; kind: Exclude<PreviewKind, null> }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const image = useRef<HTMLImageElement>(null);
  useEffect(() => {
    if (kind === "html") return;
    const made = URL.createObjectURL(blobOf(response));
    const el = kind === "image" ? image.current : frame.current;
    if (el) el.src = made;
    return () => URL.revokeObjectURL(made);
  }, [response, kind]);
  if (kind === "html") {
    return <iframe className="body-preview" title="Response preview" sandbox="" srcDoc={response.bodyText ?? ""} />;
  }
  if (kind === "image") return <img ref={image} className="body-preview-image" alt="Response image" />;
  return <iframe ref={frame} className="body-preview" title="Response preview" />;
}
