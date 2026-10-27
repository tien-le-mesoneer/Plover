import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VarsDto } from "../api/types";
import { useVars } from "../store/vars";
import { varHoverAt, type TextModel } from "./varDecorations";

const addAction = vi.fn(() => ({ dispose: vi.fn() }));
const disposable = () => ({ dispose: vi.fn() });

vi.mock("./monaco", () => {
  const model = {
    uri: { toString: () => "inmemory://1" },
    getValue: () => "",
    getOffsetAt: () => 0,
    getPositionAt: () => ({ lineNumber: 1, column: 1 }),
    dispose: vi.fn(),
  };
  const instance = {
    addCommand: vi.fn(),
    addAction,
    onDidChangeModelContent: vi.fn(disposable),
    onMouseUp: vi.fn(disposable),
    getModel: () => model,
    createDecorationsCollection: () => ({ set: vi.fn(), clear: vi.fn() }),
    getValue: () => "",
    setValue: vi.fn(),
    updateOptions: vi.fn(),
    trigger: vi.fn(),
    dispose: vi.fn(),
  };
  return {
    monaco: {
      KeyMod: { CtrlCmd: 2048, Shift: 1024 },
      KeyCode: { Enter: 3, KeyK: 41, KeyC: 33 },
      editor: { create: vi.fn(() => instance), setModelLanguage: vi.fn(), MouseTargetType: { CONTENT_TEXT: 6 } },
      languages: { registerHoverProvider: vi.fn(), registerCompletionItemProvider: vi.fn() },
    },
    currentTheme: () => "vs",
  };
});

afterEach(() => cleanup());

describe("editor overlays", () => {
  it("suggest and hover widgets are not clipped by the pane", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    const { monaco } = await import("./monaco");
    render(<CodeEditor value="" language="json" />);
    expect(monaco.editor.create).toHaveBeenLastCalledWith(expect.anything(), expect.objectContaining({ fixedOverflowWidgets: true }));
  });
});

describe("body editor value copy", () => {
  it("monaco_action_registered", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    useVars.setState({ byTab: {} });
    render(<CodeEditor value="" language="json" varTabKey="c1:a.bru" />);
    expect(addAction).toHaveBeenCalledWith(
      expect.objectContaining({ id: "plover.copyVarValue", keybindings: [2048 | 1024 | 33] }),
    );
    const dto: VarsDto = {
      vars: [{ name: "host", source: "environment", value: "api.test", masked: false, truncated: false, shadows: [] }],
      setLater: [],
    };
    const m: TextModel = { getValue: () => "{{host}}", getPositionAt: () => ({ lineNumber: 1, column: 1 }) };
    expect(varHoverAt(m, 3, dto, null)!.markdown).toMatch(/(⌘⇧C|Ctrl\+Shift\+C) copies the value/);
  });
});
