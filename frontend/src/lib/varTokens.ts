import type { VarsDto } from "../api/types";

export type VarClass = "defined" | "setLater" | "undefined";

export interface VarToken {
  start: number;
  end: number;
  name: string;
}

/** What the hover card shows for one `{{name}}`. */
export interface VarInfo {
  kind: VarClass;
  /** `null` when masked or not defined. */
  value: string | null;
  masked: boolean;
  truncated: boolean;
  /** `environment`, `folder`, … (`""` when not defined). */
  source: string;
  shadows: string[];
  /** For `setLater`: the file whose script sets it. */
  setBy?: string;
}

/** `{{name}}` placeholders as core finds them: up to the first `}`, which must start `}}`. */
export function tokenize(text: string): VarToken[] {
  const out: VarToken[] = [];
  let from = 0;
  for (;;) {
    const start = text.indexOf("{{", from);
    if (start < 0) return out;
    const close = text.indexOf("}", start + 2);
    if (close > start + 2 && text.startsWith("}}", close)) {
      const name = text.slice(start + 2, close).trim();
      if (name) out.push({ start, end: close + 2, name });
      from = close + 2;
    } else {
      from = start + 1;
    }
  }
}

/** `a.b[0]` → `a`; the root decides the class unless the whole name exists (`process.env.X`). */
const root = (name: string) => name.split(/[.[]/, 1)[0] ?? name;

export function classify(name: string, vars: VarsDto | undefined): VarClass | null {
  return explain(name, vars)?.kind ?? null;
}

export function explain(name: string, vars: VarsDto | undefined): VarInfo | null {
  if (!vars) return null;
  const entry = vars.vars.find((v) => v.name === name) ?? vars.vars.find((v) => v.name === root(name));
  if (entry) {
    return {
      kind: "defined",
      value: entry.value,
      masked: entry.masked,
      truncated: entry.truncated,
      source: entry.source,
      shadows: entry.shadows,
    };
  }
  const later = vars.setLater.find((s) => s.name === name || s.name === root(name));
  const none = { value: null, masked: false, truncated: false, source: "", shadows: [] };
  return later ? { kind: "setLater", ...none, setBy: later.by } : { kind: "undefined", ...none };
}
