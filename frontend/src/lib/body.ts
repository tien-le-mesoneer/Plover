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

/** The response viewer renders at most this many characters until "Show all". */
export const RENDER_CAP = 1 << 20;

/** The part of a response body the editor shows; Copy and Save always use the whole body. */
export function renderSlice(text: string, cap = RENDER_CAP): { shown: string; capped: boolean; total: number } {
  if (text.length <= cap) return { shown: text, capped: false, total: text.length };
  return { shown: text.slice(0, cap), capped: true, total: text.length };
}

/** An array or object in JSON text: its 1-based line range and how many direct entries it has. */
export interface JsonContainer {
  kind: "array" | "object";
  startLine: number;
  endLine: number;
  count: number;
}

/**
 * Every closed array and object in JSON text, in closing order. One pass over the text;
 * strings are skipped. A mismatched bracket gives `[]`; containers left open at the end
 * (a body cut at the render cap) are left out.
 */
export function jsonContainers(text: string): JsonContainer[] {
  const out: JsonContainer[] = [];
  const stack: { kind: JsonContainer["kind"]; line: number; count: number }[] = [];
  let line = 1;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\n") {
      line++;
      continue;
    }
    if (c === '"') {
      // The string is one entry of its container when it is not a key (keys are counted by their value).
      i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\") i++;
        else if (text[i] === "\n") line++;
        i++;
      }
    }
    const top = stack[stack.length - 1];
    if (c === "{" || c === "[") {
      if (top && top.count === 0) top.count = 1;
      stack.push({ kind: c === "[" ? "array" : "object", line, count: 0 });
    } else if (c === "}" || c === "]") {
      const frame = stack.pop();
      if (!frame || frame.kind !== (c === "]" ? "array" : "object")) return [];
      out.push({ kind: frame.kind, startLine: frame.line, endLine: line, count: frame.count });
    } else if (c === ",") {
      if (top) top.count++;
    } else if (top && top.count === 0 && c !== " " && c !== "\t" && c !== "\r") {
      top.count = 1;
    }
  }
  return out;
}

/** Arrays and objects with more than `threshold` direct entries, as 1-based line ranges, outermost first. */
export function foldTargets(
  containers: JsonContainer[],
  threshold = 50,
): Array<{ startLine: number; endLine: number }> {
  return containers
    .filter((c) => c.count > threshold)
    .map(({ startLine, endLine }) => ({ startLine, endLine }))
    .sort((a, b) => a.startLine - b.startLine);
}

/** "12 items", "1 key": the note a folded array or object shows. */
export function countLabel(c: Pick<JsonContainer, "kind" | "count">): string {
  const noun = c.kind === "array" ? "item" : "key";
  return `${c.count} ${noun}${c.count === 1 ? "" : "s"}`;
}
