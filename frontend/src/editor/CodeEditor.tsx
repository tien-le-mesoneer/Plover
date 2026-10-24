import { useEffect, useRef } from "react";
import { currentTheme, monaco } from "./monaco";

/** The `--font-mono` stack from styles.css. */
export const MONO_FONT = '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace';

export interface CodeEditorProps {
  value: string;
  language: string;
  readOnly?: boolean;
  onChange?: (value: string) => void;
  /** Editor height in CSS units; defaults to filling the parent. */
  height?: string;
  /** Called with the editor once it exists (find widget, folding, value clicks). */
  onReady?: (editor: monaco.editor.IStandaloneCodeEditor) => void;
}

/** A thin wrapper around a Monaco editor instance. */
export function CodeEditor({ value, language, readOnly, onChange, height, onReady }: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const changeHandler = useRef(onChange);
  const readyHandler = useRef(onReady);

  useEffect(() => {
    changeHandler.current = onChange;
    readyHandler.current = onReady;
  }, [onChange, onReady]);

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
    const sub = instance.onDidChangeModelContent(() => changeHandler.current?.(instance.getValue()));
    editor.current = instance;
    readyHandler.current?.(instance);
    return () => {
      sub.dispose();
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

  useEffect(() => {
    const model = editor.current?.getModel();
    if (model) monaco.editor.setModelLanguage(model, language);
  }, [language]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly });
  }, [readOnly]);

  return <div ref={host} style={{ height: height ?? "100%", width: "100%" }} />;
}
