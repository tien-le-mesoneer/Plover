import type { monaco as Monaco } from "../editor/monaco";
import type { VarsDto } from "../api/types";
import { matchScore } from "./fuzzy";
import { sourceLabel } from "./varText";

/** A name to offer after `{{`; never a value. */
export interface VarSuggestion {
  name: string;
  /** The scope (`Environment: Dev`, `Runtime`, …) or `set later`. */
  label: string;
}

const MAX_SUGGESTIONS = 50;
/** Characters a partly typed name may contain (`a.b`, `items[0]`, `x-y`). */
const NAME_CHAR = /[\w.\-[\]$]/;

/**
 * Variable names for `query`: every scope `/vars` reports except the process
 * environment, plus names scripts set later. Names starting with the query come first
 * (alphabetical), then other subsequence matches (best first); at most 50.
 */
export function suggestions(vars: VarsDto | undefined, query: string, env: string | null): VarSuggestion[] {
  if (!vars) return [];
  const all: VarSuggestion[] = [];
  const seen = new Set<string>();
  for (const v of vars.vars) {
    if (v.source === "processEnv" || seen.has(v.name)) continue;
    seen.add(v.name);
    all.push({ name: v.name, label: sourceLabel(v.source, env) });
  }
  for (const s of vars.setLater) {
    if (seen.has(s.name)) continue;
    seen.add(s.name);
    all.push({ name: s.name, label: "set later" });
  }
  const q = query.trim().toLowerCase();
  const prefix = all.filter((s) => s.name.toLowerCase().startsWith(q)).sort((a, b) => a.name.localeCompare(b.name));
  const fuzzy = all
    .filter((s) => !s.name.toLowerCase().startsWith(q))
    .map((s) => ({ s, score: matchScore(q, s.name) }))
    .filter((x): x is { s: VarSuggestion; score: number } => x.score !== null)
    .sort((a, b) => b.score - a.score || a.s.name.localeCompare(b.s.name))
    .map((x) => x.s);
  return [...prefix, ...fuzzy].slice(0, MAX_SUGGESTIONS);
}

/** The `{{…` the caret is in, while its name is still being typed; `null` otherwise. */
export function openToken(text: string, caret: number): { start: number; query: string } | null {
  const start = text.lastIndexOf("{{", caret - 2);
  if (start < 0 || start + 2 > caret) return null;
  const query = text.slice(start + 2, caret);
  if (query.includes("}") || query.includes("{") || /\s/.test(query)) return null;
  return { start, query };
}

/** Writes `{{name}}` over the open token, adding `}}` only if it is missing; the caret goes after it. */
export function acceptSuggestion(text: string, caret: number, name: string): { text: string; caret: number } {
  const open = openToken(text, caret);
  const start = open ? open.start : caret;
  let end = caret;
  while (end < text.length && NAME_CHAR.test(text[end]!)) end++;
  const closed = text.startsWith("}}", end);
  const before = `${text.slice(0, start)}{{${name}}}`;
  return { text: before + text.slice(closed ? end + 2 : end), caret: before.length };
}

/**
 * Offers variable names after `{{` in the body editors (one provider per body language;
 * Monaco providers are global). `getVars` answers for the model being edited.
 */
export function registerVarCompletion(
  monaco: typeof Monaco,
  getVars: (model: Monaco.editor.ITextModel) => { vars: VarsDto | undefined; env: string | null } | null,
): void {
  for (const language of ["json", "plaintext", "xml"]) {
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: ["{"],
      provideCompletionItems(model, position) {
        const found = getVars(model);
        const text = model.getValue();
        const caret = model.getOffsetAt(position);
        const open = found ? openToken(text, caret) : null;
        if (!found || !open) return { suggestions: [] };
        let end = caret;
        while (end < text.length && NAME_CHAR.test(text[end]!)) end++;
        const closed = text.startsWith("}}", end);
        const from = model.getPositionAt(open.start + 2);
        const to = model.getPositionAt(closed ? end + 2 : end);
        const range = { startLineNumber: from.lineNumber, startColumn: from.column, endLineNumber: to.lineNumber, endColumn: to.column };
        return {
          suggestions: suggestions(found.vars, open.query, found.env).map((s, i) => ({
            label: s.name,
            kind: monaco.languages.CompletionItemKind.Variable,
            detail: s.label,
            insertText: `${s.name}}}`,
            // Keep our order (prefix matches first) instead of Monaco's.
            sortText: String(i).padStart(3, "0"),
            filterText: s.name,
            range,
          })),
        };
      },
    });
  }
}
