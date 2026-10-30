/** One token of a JSON body: whitespace, a string, a number, a literal, a `{{variable}}` or punctuation. */
const TOKEN =
  // JSON strings may not hold raw control characters, so the class names them.
  // eslint-disable-next-line no-control-regex
  /\s+|"(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?(?![\w.])|(?:true|false|null)(?!\w)|\{\{[^{}]*\}\}|[{}[\],:]/y;

interface Token {
  text: string;
  start: number;
}

/** Where a JSON body stops being JSON, as offsets into the text. */
export interface JsonProblem {
  start: number;
  end: number;
  message: string;
}

const isScalar = (t: string | undefined) => t !== undefined && !"{}[],:".includes(t);

/**
 * Reads a JSON body whose values and keys may be `{{variables}}` (also unquoted ones, as
 * in `"id": {{id}}`) and lays it out with 2-space indents, or says where it is not JSON.
 */
function parse(text: string): { pretty: string } | { problem: JsonProblem } {
  const list: Token[] = [];
  TOKEN.lastIndex = 0;
  while (TOKEN.lastIndex < text.length) {
    const at = TOKEN.lastIndex;
    const m = TOKEN.exec(text);
    if (!m) return { problem: { start: at, end: at + 1, message: "Unexpected character" } };
    if (m[0].trim()) list.push({ text: m[0], start: at });
  }
  const last = text.trimEnd().length;
  let problem: JsonProblem | null = null;
  // At the token, or on the last character when the text ends too early.
  const fail = (t: Token | undefined, message: string) => {
    problem = t ? { start: t.start, end: t.start + t.text.length, message } : { start: Math.max(0, last - 1), end: last, message };
    return null;
  };
  let i = 0;
  const value = (indent: string): string | null => {
    const tok = list[i++];
    const t = tok?.text;
    if (t === "{" || t === "[") {
      const close = t === "{" ? "}" : "]";
      if (list[i]?.text === close) {
        i++;
        return t + close;
      }
      const inner = `${indent}  `;
      const items: string[] = [];
      for (;;) {
        let key = "";
        if (t === "{") {
          const k = list[i++];
          if (!k || !(k.text.startsWith('"') || k.text.startsWith("{{"))) return fail(k, "Expected a property name");
          const colon = list[i++];
          if (colon?.text !== ":") return fail(colon, "Expected ':'");
          key = `${k.text}: `;
        }
        const v = value(inner);
        if (v === null) return null;
        items.push(inner + key + v);
        const next = list[i++];
        if (next?.text === close) break;
        if (next?.text !== ",") return fail(next, `Expected ',' or '${close}'`);
        if (list[i]?.text === close) return fail(next, "Trailing comma");
      }
      return `${t}\n${items.join(",\n")}\n${indent}${close}`;
    }
    return isScalar(t) ? t! : fail(tok, "Expected a value");
  };
  const out = value("");
  if (out !== null && i < list.length) fail(list[i], "Expected end of JSON");
  if (problem) return { problem };
  return { pretty: text.endsWith("\n") ? `${out}\n` : out! };
}

/**
 * Re-indents a JSON body with 2 spaces. Strings, numbers and `{{variables}}` (also
 * unquoted ones, as in `"id": {{id}}`) are kept as written, so nothing is unescaped or
 * rounded. A final newline is kept. Null when the text is not JSON.
 */
export function prettifyJson(text: string): string | null {
  if (!text.trim()) return null;
  const parsed = parse(text);
  return "pretty" in parsed ? parsed.pretty : null;
}

/** Where a JSON body is not JSON; null when it is, or when it is empty. */
export function jsonProblem(text: string): JsonProblem | null {
  if (!text.trim()) return null;
  const parsed = parse(text);
  return "problem" in parsed ? parsed.problem : null;
}
