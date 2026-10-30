/** One token of a JSON body: whitespace, a string, a number, a literal, a `{{variable}}` or punctuation. */
const TOKEN =
  // JSON strings may not hold raw control characters, so the class names them.
  // eslint-disable-next-line no-control-regex
  /\s+|"(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?(?![\w.])|(?:true|false|null)(?!\w)|\{\{[^{}]*\}\}|[{}[\],:]/y;

function tokens(text: string): string[] | null {
  const out: string[] = [];
  TOKEN.lastIndex = 0;
  while (TOKEN.lastIndex < text.length) {
    const m = TOKEN.exec(text);
    if (!m) return null;
    if (m[0].trim()) out.push(m[0]);
  }
  return out;
}

const isScalar = (t: string | undefined) => t !== undefined && !"{}[],:".includes(t);

/**
 * Re-indents a JSON body with 2 spaces. Strings, numbers and `{{variables}}` (also
 * unquoted ones, as in `"id": {{id}}`) are kept as written, so nothing is unescaped or
 * rounded. A final newline is kept. Null when the text is not JSON.
 */
export function prettifyJson(text: string): string | null {
  const list = tokens(text);
  if (!list || list.length === 0) return null;
  let i = 0;
  const value = (indent: string): string | null => {
    const t = list[i++];
    if (t === "{" || t === "[") {
      const close = t === "{" ? "}" : "]";
      if (list[i] === close) {
        i++;
        return t + close;
      }
      const inner = `${indent}  `;
      const items: string[] = [];
      for (;;) {
        let key = "";
        if (t === "{") {
          const k = list[i++];
          if (!k || !(k.startsWith('"') || k.startsWith("{{")) || list[i++] !== ":") return null;
          key = `${k}: `;
        }
        const v = value(inner);
        if (v === null) return null;
        items.push(inner + key + v);
        const next = list[i++];
        if (next === close) break;
        if (next !== ",") return null;
      }
      return `${t}\n${items.join(",\n")}\n${indent}${close}`;
    }
    return isScalar(t) ? t! : null;
  };
  const out = value("");
  if (out === null || i !== list.length) return null;
  return text.endsWith("\n") ? `${out}\n` : out;
}
