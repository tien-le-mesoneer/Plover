// Maps a position in JSON text to the value under it and its JSONPath, in one linear scan
// (no parse tree), so a click in a large pretty-printed body stays cheap.

export interface ValueAt {
  path: string;
  value: unknown;
  /** Offsets of the value in the text: `[start, end)`. */
  start: number;
  end: number;
}

const IDENT = /^[A-Za-z_$][\w$]*$/;

function keySegment(key: string): string {
  return IDENT.test(key) ? `.${key}` : `['${key.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}']`;
}

interface Frame {
  kind: "obj" | "arr";
  path: string;
  start: number;
  /** Next array index, or the key read for the next object value. */
  index: number;
  key: string | null;
  /** The value under the offset is this container (the offset is on a bracket or its key). */
  hit: boolean;
}

/**
 * The innermost value at `offset` in `text`: a scalar under the cursor, the value of a key
 * under the cursor, or a container whose bracket is under it. `null` on whitespace,
 * commas and colons, outside any value, or when `text` is not JSON.
 */
export function jsonPathAt(text: string, offset: number): ValueAt | null {
  const stack: Frame[] = [];
  /** A key under the cursor: report the value that follows it. */
  let keyHit = false;
  let i = 0;
  const n = text.length;

  const childPath = (): string => {
    const top = stack[stack.length - 1];
    if (!top) return "$";
    return top.kind === "arr" ? `${top.path}[${top.index}]` : `${top.path}${keySegment(top.key ?? "")}`;
  };
  const done = (path: string, start: number, end: number): ValueAt | null => {
    try {
      return { path, value: JSON.parse(text.slice(start, end)), start, end };
    } catch {
      return null;
    }
  };
  /** After a value in the current container: advance its index, clear its key. */
  const next = () => {
    const top = stack[stack.length - 1];
    if (!top) return;
    if (top.kind === "arr") top.index++;
    else top.key = null;
  };

  while (i < n) {
    const c = text[i]!;
    if (c === " " || c === "\n" || c === "\r" || c === "\t" || c === "," || c === ":") {
      i++;
      continue;
    }
    const top = stack[stack.length - 1];
    const expectingKey = top?.kind === "obj" && top.key === null;
    if (c === '"') {
      const start = i;
      i++;
      while (i < n && text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
      if (i >= n) return null;
      i++;
      const end = i;
      if (expectingKey) {
        try {
          top.key = JSON.parse(text.slice(start, end)) as string;
        } catch {
          return null;
        }
        if (offset >= start && offset < end) keyHit = true;
        continue;
      }
      if (keyHit || (offset >= start && offset < end)) return done(childPath(), start, end);
      next();
      continue;
    }
    if (c === "{" || c === "[") {
      if (expectingKey) return null;
      stack.push({
        kind: c === "{" ? "obj" : "arr",
        path: childPath(),
        start: i,
        index: 0,
        key: null,
        hit: keyHit || offset === i,
      });
      keyHit = false;
      i++;
      continue;
    }
    if (c === "}" || c === "]") {
      const frame = stack.pop();
      if (!frame) return null;
      i++;
      if (frame.hit || offset === i - 1) return done(frame.path, frame.start, i);
      next();
      continue;
    }
    // A number, true, false or null.
    const start = i;
    while (i < n && !" \n\r\t,:]}".includes(text[i]!)) i++;
    if (i === start || expectingKey) return null;
    if (keyHit || (offset >= start && offset < i)) return done(childPath(), start, i);
    next();
  }
  return null;
}
