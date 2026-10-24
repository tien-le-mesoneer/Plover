// Monaco is bundled locally (no CDN loader): the editor core, all editor
// features, and only the languages the UI shows. monaco 0.57 loads its workers
// through `new URL(..., import.meta.url)`, which Vite bundles as same-origin files.
import * as monaco from "monaco-editor/editor/editor.api.js";
// Editor contributions (codelens, inlay hints, suggest, dnd, code actions, ...)
// and the services they register; `editor.main` imports the same list plus
// every language. Without it, creating an editor throws "depends on UNKNOWN
// service ICodeLensCache" and similar.
import "monaco-editor/features/register.all.js";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";
import JsonWorker from "monaco-editor/languages/features/json/json.worker.js?worker";
import "monaco-editor/languages/features/json/register.js";
import "monaco-editor/languages/definitions/xml/register.js";
import "monaco-editor/languages/definitions/html/register.js";
import "monaco-editor/languages/definitions/javascript/register.js";
import { themeFor, watchColorScheme, type ThemeName } from "./theme";

// Monaco creates its main editor worker from a `new URL(...)` that is not
// passed straight to `new Worker`, so Vite inlines it as an unusable data: URL
// (an ErrorEvent per editor). Hand Monaco Vite-bundled workers instead; once
// `getWorker` is defined it must cover every worker label we use.
self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    return label === "json" ? new JsonWorker() : new EditorWorker();
  },
};

// Themes matching the app's colour tokens (styles.css), so editors don't stand
// out as a lighter or darker block.
monaco.editor.defineTheme("plover-light", {
  base: "vs",
  inherit: true,
  rules: [],
  // Same values as --bg / --bg-2 in styles.css (Monaco cannot read CSS variables).
  colors: { "editor.background": "#ffffff", "editor.lineHighlightBackground": "#f6f7f8" },
});
monaco.editor.defineTheme("plover-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: { "editor.background": "#16181c", "editor.lineHighlightBackground": "#1d2025" },
});

const scheme = typeof window !== "undefined" ? window.matchMedia?.("(prefers-color-scheme: dark)") : undefined;
let current: ThemeName = themeFor(scheme?.matches ?? false);
// Monaco's theme is global: one listener follows macOS/Windows appearance changes live.
if (scheme) {
  watchColorScheme(scheme, (theme) => {
    current = theme;
    monaco.editor.setTheme(theme);
  });
}

/** The theme for newly created editors. */
export const currentTheme = (): ThemeName => current;

export { monaco };
