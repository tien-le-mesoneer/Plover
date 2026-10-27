import { useEffect, useRef } from "react";
import { useTabs } from "../store/tabs";
import { useVars } from "../store/vars";
import { useWorkspace } from "../store/workspace";
import { currentTheme, monaco } from "./monaco";
import { copyResolved } from "../lib/copyResolved";
import { copiedValueNote } from "../lib/varText";
import { explain } from "../lib/varTokens";
import { varDecorations, varHoverAt, varTokenAt } from "./varDecorations";

/** The `--font-mono` stack from styles.css. */
export const MONO_FONT = '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace';

export interface CodeEditorProps {
  value: string;
  language: string;
  readOnly?: boolean;
  onChange?: (value: string) => void;
  /** Editor height in CSS units; defaults to filling the parent. */
  height?: string;
  /** Called with the editor once it exists (find widget, folding). */
  onReady?: (editor: monaco.editor.IStandaloneCodeEditor) => void;
  /**
   * A click on the text, or Enter in a read-only editor: the offset in the text and the
   * screen point under it (for a menu).
   */
  onPick?: (offset: number, at: { x: number; y: number }) => void;
  /** 1-based start lines of regions to fold each time the value is set (large JSON arrays). */
  foldLines?: number[];
  /** The request tab whose `{{variables}}` this editor underlines and explains (bodies only). */
  varTabKey?: string;
  /** The note of the last ⌘/Ctrl+Shift+C value copy (for a live region). */
  onNote?: (note: string) => void;
}

/** Body editor models whose `{{variables}}` are explained, by model URI → request tab key. */
const varModels = new Map<string, string>();
let varHoverReady = false;

/** One hover provider per body language, shared by all editors (Monaco providers are global). */
function ensureVarHover() {
  if (varHoverReady) return;
  varHoverReady = true;
  for (const language of ["json", "plaintext", "xml"]) {
    monaco.languages.registerHoverProvider(language, {
      provideHover(model, position) {
        const key = varModels.get(model.uri.toString());
        if (!key) return null;
        const tab = useTabs.getState().tabs.find((t) => t.key === key);
        const env = tab ? (useWorkspace.getState().activeEnv[tab.collectionId] ?? null) : null;
        const hover = varHoverAt(model, model.getOffsetAt(position), useVars.getState().byTab[key], env);
        return hover ? { range: hover.range, contents: [{ value: hover.markdown }] } : null;
      },
    });
  }
}

/**
 * Repaints after edits (debounced) and when the tab's `/vars` answer changes, and adds
 * ⌘/Ctrl+Shift+C: copy the value of the variable under the cursor.
 */
function decorateVars(
  instance: monaco.editor.IStandaloneCodeEditor,
  tabKey: string,
  note: { current: ((note: string) => void) | undefined },
): () => void {
  const model = instance.getModel();
  if (!model) return () => {};
  // addAction, not addCommand: commands are shared by all editors on the page.
  const copyAction = instance.addAction({
    id: "plover.copyVarValue",
    label: "Copy variable value",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyC],
    run: (ed) => {
      const m = ed.getModel();
      const position = ed.getPosition();
      if (!m || !position) return;
      const token = varTokenAt(m.getValue(), m.getOffsetAt(position));
      if (!token || explain(token.name, useVars.getState().byTab[tabKey])?.kind !== "defined") return;
      void copyResolved(tabKey, `{{${token.name}}}`, copiedValueNote(token.name)).then((n) => note.current?.(n));
    },
  });
  ensureVarHover();
  const uri = model.uri.toString();
  varModels.set(uri, tabKey);
  const marks = instance.createDecorationsCollection();
  const paint = () => marks.set(varDecorations(model, useVars.getState().byTab[tabKey]));
  paint();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const edits = instance.onDidChangeModelContent(() => {
    clearTimeout(timer);
    timer = setTimeout(paint, 100);
  });
  const unsubscribe = useVars.subscribe((s, prev) => {
    if (s.byTab[tabKey] !== prev.byTab[tabKey]) paint();
  });
  return () => {
    clearTimeout(timer);
    copyAction.dispose();
    edits.dispose();
    unsubscribe();
    marks.clear();
    varModels.delete(uri);
  };
}

/** A thin wrapper around a Monaco editor instance. */
export function CodeEditor({ value, language, readOnly, onChange, height, onReady, onPick, foldLines, varTabKey, onNote }: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const changeHandler = useRef(onChange);
  const readyHandler = useRef(onReady);
  const pickHandler = useRef(onPick);
  const noteHandler = useRef(onNote);

  useEffect(() => {
    changeHandler.current = onChange;
    readyHandler.current = onReady;
    pickHandler.current = onPick;
    noteHandler.current = onNote;
  }, [onChange, onReady, onPick, onNote]);

  useEffect(() => {
    if (!host.current) return;
    const instance = monaco.editor.create(host.current, {
      value,
      language,
      readOnly,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      fontSize: 13,
      fontFamily: MONO_FONT,
      tabSize: 2,
      theme: currentTheme(),
    });
    // Cmd/Ctrl+Enter sends the request even while typing in an editor (Monaco
    // otherwise inserts a line); App listens for this event.
    instance.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () =>
      window.dispatchEvent(new Event("plover:send")),
    );
    // Cmd/Ctrl+K opens quick open; Monaco's own Cmd+K chords are given up for it.
    instance.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyK, () =>
      window.dispatchEvent(new Event("plover:quick-open")),
    );
    const sub = instance.onDidChangeModelContent(() => changeHandler.current?.(instance.getValue()));
    const pick = (position: monaco.Position | null) => {
      const model = instance.getModel();
      const handler = pickHandler.current;
      if (!model || !position || !handler) return;
      const point = instance.getScrolledVisiblePosition(position);
      const box = instance.getDomNode()?.getBoundingClientRect();
      handler(model.getOffsetAt(position), {
        x: (box?.left ?? 0) + (point?.left ?? 0),
        y: (box?.top ?? 0) + (point?.top ?? 0) + (point?.height ?? 0),
      });
    };
    const clicks = instance.onMouseUp((e) => {
      if (e.target.type === monaco.editor.MouseTargetType.CONTENT_TEXT && instance.getSelection()?.isEmpty()) {
        pick(e.target.position);
      }
    });
    // Enter on a value opens the same menu from the keyboard (read-only editors only).
    // addAction, not addCommand: commands are shared by all editors on the page.
    const enter = readOnly
      ? instance.addAction({
          id: "plover.pickValue",
          label: "Value actions",
          keybindings: [monaco.KeyCode.Enter],
          run: () => pick(instance.getPosition()),
        })
      : undefined;
    editor.current = instance;
    readyHandler.current?.(instance);
    const stopVars = varTabKey ? decorateVars(instance, varTabKey, noteHandler) : undefined;
    return () => {
      stopVars?.();
      sub.dispose();
      clicks.dispose();
      enter?.dispose();
      instance.getModel()?.dispose();
      instance.dispose();
      editor.current = null;
    };
    // Created once; value/language/readOnly updates are applied below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const instance = editor.current;
    if (instance && instance.getValue() !== value) instance.setValue(value);
  }, [value]);

  // Folded once per value (a new array with the same lines must not undo the user's unfold).
  const foldKey = foldLines?.join(",") ?? "";
  useEffect(() => {
    const instance = editor.current;
    if (!instance || !foldKey) return;
    // `editor.fold` waits for the folding model; selectionLines are 0-based.
    const lines = foldKey.split(",").map((l) => Number(l) - 1);
    instance.trigger("plover", "editor.fold", { levels: 1, selectionLines: lines });
  }, [value, foldKey]);

  useEffect(() => {
    const model = editor.current?.getModel();
    if (model) monaco.editor.setModelLanguage(model, language);
  }, [language]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly });
  }, [readOnly]);

  return <div ref={host} style={{ height: height ?? "100%", width: "100%" }} />;
}
