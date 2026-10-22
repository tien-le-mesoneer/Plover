import type { ResponseDto } from "../api/types";

export function ResponseViewer({ response, sending }: { response?: ResponseDto; sending: boolean }) {
  if (sending) return <p className="muted">Sending…</p>;
  if (!response) return <p className="muted">Send the request to see the response.</p>;
  return (
    <pre>
      {response.status} {response.bodyText ?? ""}
    </pre>
  );
}
