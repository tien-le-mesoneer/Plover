import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VarsDto } from "../api/types";
import { useVars } from "../store/vars";
import { varHoverAt, type TextModel } from "./varDecorations";

const addAction = vi.fn(() => ({ dispose: vi.fn() }));
const disposable = () => ({ dispose: vi.fn() });
const setModelMarkers = vi.fn();
/** The text of the mocked model (a hoisted mock cannot close over a plain `let`). */
const modelText = { value: "" };

vi.mock("./monaco", () => {
  const model = {
    uri: { toString: () => "inmemory://1" },
    getValue: () => modelText.value,
    getLanguageId: () => "json",
    onDidChangeLanguage: vi.fn(disposable),
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
      KeyMod: { CtrlCmd: 2048, Shift: 1024, Alt: 512 },
      KeyCode: { Enter: 3, KeyK: 41, KeyC: 33, KeyF: 36 },
      editor: {
        create: vi.fn(() => instance),
        setModelLanguage: vi.fn(),
        setModelMarkers,
        MouseTargetType: { CONTENT_TEXT: 6 },
      },
      MarkerSeverity: { Error: 8 },
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

describe("body editor format", () => {
  it("shift_alt_f_runs_on_format", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    const onFormat = vi.fn();
    addAction.mockClear();
    render(<CodeEditor value="" language="json" onFormat={onFormat} />);
    const calls = addAction.mock.calls as unknown as [{ id: string; keybindings: number[]; run: () => void }][];
    const format = calls.map((c) => c[0]).find((a) => a.id === "plover.format")!;
    expect(format.keybindings).toEqual([1024 | 512 | 36]);
    format.run();
    expect(onFormat).toHaveBeenCalled();
  });
});

describe("response value menu", () => {
  it("enter_opens_the_menu_only_from_the_text", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    addAction.mockClear();
    render(<CodeEditor value="" language="json" readOnly onPick={vi.fn()} />);
    const calls = addAction.mock.calls as unknown as [{ id: string; keybindings: number[]; keybindingContext?: string }][];
    const pick = calls.map((c) => c[0]).find((a) => a.id === "plover.pickValue")!;
    expect(pick.keybindings).toEqual([3]);
    // Enter in the find widget must go to the next match, not open Copy value / Copy path.
    expect(pick.keybindingContext).toBe("editorTextFocus");
  });
});

describe("json body check", () => {
  it("marks a real error but not an unquoted {{variable}}", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    modelText.value = '{"id": {{id}}}';
    setModelMarkers.mockClear();
    const view = render(<CodeEditor value={modelText.value} language="json" />);
    expect(setModelMarkers).toHaveBeenLastCalledWith(expect.anything(), "plover-json", []);
    view.unmount();

    modelText.value = '{"id": {{id}},}';
    render(<CodeEditor value={modelText.value} language="json" />);
    expect(setModelMarkers).toHaveBeenLastCalledWith(expect.anything(), "plover-json", [
      expect.objectContaining({ message: "Trailing comma", severity: 8 }),
    ]);
    modelText.value = "";
  });

  it("does not check a read-only editor (the response)", async () => {
    const { CodeEditor } = await import("./CodeEditor");
    setModelMarkers.mockClear();
    render(<CodeEditor value="{" language="json" readOnly />);
    expect(setModelMarkers).not.toHaveBeenCalled();
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
