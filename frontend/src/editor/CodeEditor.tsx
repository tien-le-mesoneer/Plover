import { useEffect, useRef } from "react";
import { monaco } from "./monaco";

export interface CodeEditorProps {
  value: string;
  language: string;
  readOnly?: boolean;
  onChange?: (value: string) => void;
  /** Editor height in CSS units; defaults to filling the parent. */
  height?: string;
}

/** A thin wrapper around a Monaco editor instance. */
export function CodeEditor({ value, language, readOnly, onChange, height }: CodeEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const changeHandler = useRef(onChange);

  useEffect(() => {
    changeHandler.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!host.current) return;
    const dark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
    const instance = monaco.editor.create(host.current, {
      value,
      language,
      readOnly,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      fontSize: 13,
      tabSize: 2,
      theme: dark ? "vs-dark" : "vs",
    });
    const sub = instance.onDidChangeModelContent(() => changeHandler.current?.(instance.getValue()));
    editor.current = instance;
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
