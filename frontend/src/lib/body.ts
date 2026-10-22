import type { BodyDto, HttpRequestDto } from "../api/types";

type Mode = Exclude<BodyDto["mode"], "unsupported">;

function empty(mode: Mode): BodyDto {
  switch (mode) {
    case "json":
    case "text":
    case "xml":
      return { mode, text: "" };
    case "formUrlEncoded":
    case "multipartForm":
      return { mode, fields: [] };
    case "none":
      return { mode: "none" };
  }
}

/**
 * Bruno keeps one body per mode. Switching parks the current body in
 * `otherBodies` and restores the target mode's body (or starts it empty).
 */
export function switchBodyMode(draft: HttpRequestDto, mode: Mode): HttpRequestDto {
  if (draft.body.mode === mode) return draft;
  const current = draft.body;
  const parked = draft.otherBodies.filter((b) => b.mode !== current.mode);
  if (current.mode !== "none") parked.push(current);
  const restored = parked.find((b) => b.mode === mode);
  return {
    ...draft,
    body: restored ?? empty(mode),
    otherBodies: parked.filter((b) => b !== restored),
  };
}
