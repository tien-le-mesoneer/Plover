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

// Monaco creates its main editor worker from a `new URL(...)` that is not
// passed straight to `new Worker`, so Vite inlines it as an unusable data: URL
// (an ErrorEvent per editor). Hand Monaco Vite-bundled workers instead; once
// `getWorker` is defined it must cover every worker label we use.
self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    return label === "json" ? new JsonWorker() : new EditorWorker();
  },
};

export { monaco };
