// Monaco is bundled locally (no CDN loader). Only the editor core and the
// languages the UI shows are included; monaco 0.57 loads its workers through
// `new URL(..., import.meta.url)`, which Vite bundles as same-origin files.
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/languages/features/json/register.js";
import "monaco-editor/languages/definitions/xml/register.js";
import "monaco-editor/languages/definitions/html/register.js";
import "monaco-editor/languages/definitions/javascript/register.js";

export { monaco };
