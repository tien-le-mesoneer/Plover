import type { VarsDto } from "../api/types";
import { explain, tokenize } from "../lib/varTokens";
import { copyShortcut, hoverLines } from "../lib/varText";

/** The part of a Monaco text model these helpers read (tests pass a fake). */
export interface TextModel {
  getValue(): string;
  getPositionAt(offset: number): { lineNumber: number; column: number };
}

export interface VarDecoration {
  range: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number };
  options: { inlineClassName: string };
}

const rangeOf = (model: TextModel, start: number, end: number) => {
  const a = model.getPositionAt(start);
  const b = model.getPositionAt(end);
  return { startLineNumber: a.lineNumber, startColumn: a.column, endLineNumber: b.lineNumber, endColumn: b.column };
};

/** One inline decoration per `{{token}}`; none until the tab's first `/vars` answer. */
export function varDecorations(model: TextModel, vars: VarsDto | undefined): VarDecoration[] {
  if (!vars) return [];
  return tokenize(model.getValue()).map((t) => ({
    range: rangeOf(model, t.start, t.end),
    options: { inlineClassName: `var var-${explain(t.name, vars)!.kind.toLowerCase()}` },
  }));
}

const code = (s: string) => {
  const fence = s.includes("`") ? "``" : "`";
  return `${fence}${s.replace(/\n/g, " ")}${fence}`;
};

/** The `{{name}}` whose text covers `offset`, if any. */
export function varTokenAt(text: string, offset: number) {
  return tokenize(text).find((t) => offset >= t.start && offset <= t.end) ?? null;
}

/** The hover card as Markdown for the token at `offset`, with its range. */
export function varHoverAt(
  model: TextModel,
  offset: number,
  vars: VarsDto | undefined,
  env: string | null,
): { range: VarDecoration["range"]; markdown: string } | null {
  const token = varTokenAt(model.getValue(), offset);
  const info = token ? explain(token.name, vars) : null;
  if (!token || !info) return null;
  const lines = hoverLines(info, env).map((l) => [l.value !== undefined ? code(l.value) : "", l.note].filter(Boolean).join(" "));
  if (info.kind === "defined") lines.push(`${copyShortcut()} copies the value`);
  return { range: rangeOf(model, token.start, token.end), markdown: [code(`{{${token.name}}}`), ...lines].join("\n\n") };
}
