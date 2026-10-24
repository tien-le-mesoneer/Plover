// A small JSONPath subset for the response filter (spec C6): `$`, `.key`, `['key']`,
// `["key"]`, `[n]` (negative from the end), `[*]`, `.*` and `..key` / `..*`. Anything
// else is reported with its position; evaluation never throws.

export type JsonPathResult = { ok: true; values: unknown[] } | { ok: false; error: string; at: number };

type Step =
  | { kind: "key"; key: string; deep: boolean }
  | { kind: "index"; index: number }
  | { kind: "all"; deep: boolean };

const MAX_DEPTH = 512;
const IDENT = /[A-Za-z_$][\w$-]*/y;

class PathError extends Error {
  constructor(
    message: string,
    readonly at: number,
  ) {
    super(message);
  }
}

function parse(expr: string): Step[] {
  let i = 0;
  const steps: Step[] = [];
  const unsupported = () => new PathError(`Unsupported at ${i + 1}: ${expr.slice(i, i + 12)}`, i);
  while (i < expr.length && expr[i] === " ") i++;
  if (expr[i] !== "$") throw new PathError("A filter starts with $", i);
  i++;
  while (i < expr.length) {
    const c = expr[i];
    if (c === ".") {
      const deep = expr[i + 1] === ".";
      i += deep ? 2 : 1;
      if (expr[i] === "*") {
        steps.push({ kind: "all", deep });
        i++;
        continue;
      }
      if (deep && expr[i] === "[") {
        // `..['key']` / `..[*]`: read the bracket as a deep step.
        const step = bracket();
        if (step.kind === "index") throw unsupported();
        steps.push({ ...step, deep: true });
        continue;
      }
      IDENT.lastIndex = i;
      const m = IDENT.exec(expr);
      if (!m) throw unsupported();
      steps.push({ kind: "key", key: m[0], deep });
      i += m[0].length;
    } else if (c === "[") {
      steps.push(bracket());
    } else if (c === " ") {
      i++;
    } else {
      throw unsupported();
    }
  }
  return steps;

  function bracket(): Step {
    const start = i;
    i++; // [
    while (expr[i] === " ") i++;
    let step: Step;
    const q = expr[i];
    if (q === "'" || q === '"') {
      i++;
      let key = "";
      while (i < expr.length && expr[i] !== q) {
        if (expr[i] === "\\" && i + 1 < expr.length) i++;
        key += expr[i];
        i++;
      }
      if (expr[i] !== q) throw new PathError("Unclosed quote", start);
      i++;
      step = { kind: "key", key, deep: false };
    } else if (q === "*") {
      i++;
      step = { kind: "all", deep: false };
    } else {
      const m = /-?\d+/y;
      m.lastIndex = i;
      const found = m.exec(expr);
      if (!found) {
        i = start;
        throw unsupported();
      }
      i += found[0].length;
      step = { kind: "index", index: Number(found[0]) };
    }
    while (expr[i] === " ") i++;
    if (expr[i] !== "]") {
      i = start;
      throw unsupported();
    }
    i++;
    return step;
  }
}

function children(v: unknown): unknown[] {
  if (Array.isArray(v)) return v;
  if (v !== null && typeof v === "object") return Object.values(v);
  return [];
}

/** `v` and everything below it, in document order (pre-order), without recursion. */
function descendants(v: unknown, out: unknown[]): void {
  const stack: [unknown, number][] = [[v, 0]];
  while (stack.length) {
    const [node, depth] = stack.pop()!;
    if (depth > MAX_DEPTH) throw new PathError("The body is nested too deeply", 0);
    out.push(node);
    const kids = children(node);
    for (let k = kids.length - 1; k >= 0; k--) stack.push([kids[k], depth + 1]);
  }
}

function apply(nodes: unknown[], step: Step): unknown[] {
  let scope = nodes;
  if (step.kind !== "index" && step.deep) {
    scope = [];
    for (const n of nodes) descendants(n, scope);
  }
  const out: unknown[] = [];
  for (const n of scope) {
    if (step.kind === "key") {
      if (n !== null && typeof n === "object" && !Array.isArray(n) && Object.hasOwn(n, step.key)) {
        out.push((n as Record<string, unknown>)[step.key]);
      }
    } else if (step.kind === "index") {
      if (Array.isArray(n)) {
        const i = step.index < 0 ? n.length + step.index : step.index;
        if (i >= 0 && i < n.length) out.push(n[i]);
      }
    } else {
      // `*`; for `..*` the scope is every node below, so this lists each descendant once.
      out.push(...children(n));
    }
  }
  return out;
}

export function evaluateJsonPath(doc: unknown, expr: string): JsonPathResult {
  try {
    let nodes: unknown[] = [doc];
    for (const step of parse(expr)) nodes = apply(nodes, step);
    return { ok: true, values: nodes };
  } catch (e) {
    if (e instanceof PathError) return { ok: false, error: e.message, at: e.at };
    return { ok: false, error: e instanceof Error ? e.message : String(e), at: 0 };
  }
}

/** What the filtered view shows: one value pretty-printed, several as a JSON array. */
export function filterText(result: JsonPathResult): string {
  if (!result.ok) return "";
  const shown = result.values.length === 1 ? result.values[0] : result.values;
  return JSON.stringify(shown, null, 2) ?? "undefined";
}
