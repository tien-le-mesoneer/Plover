import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BodyDto } from "../../api/types";
import type { CodeEditorProps } from "../../editor/CodeEditor";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { BodyTab } from "./BodyTab";

/** The props the body editor was last rendered with; `ready` hands it a fake editor. */
let editorProps: CodeEditorProps | undefined;
vi.mock("../../editor/CodeEditor", () => ({
  CodeEditor: (props: CodeEditorProps) => {
    editorProps = props;
    return null;
  },
}));

let key: string;

beforeEach(() => {
  editorProps = undefined;
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    environments: { c1: [] },
    activeEnv: { c1: "local" },
  });
  useTabs.setState({ tabs: [], active: null, saveDialog: null });
  key = useTabs.getState().newScratch("c1");
});

afterEach(() => cleanup());

const json = (text: string): BodyDto => ({ mode: "json", text });

describe("JSON body prettify", () => {
  it("formats the body, keeping {{variables}}", () => {
    const onChange = vi.fn();
    render(<BodyTab tabKey={key} body={json('{"id":{{id}}}')} onChange={onChange} onModeChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Prettify JSON" }));
    expect(onChange).toHaveBeenCalledWith({ mode: "json", text: '{\n  "id": {{id}}\n}' });
  });

  it("leaves invalid JSON alone and says why", () => {
    const onChange = vi.fn();
    render(<BodyTab tabKey={key} body={json('{"a":')} onChange={onChange} onModeChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Prettify JSON" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getAllByText("Not valid JSON").length).toBeGreaterThan(0);
  });

  it("is offered for JSON bodies only", () => {
    render(<BodyTab tabKey={key} body={{ mode: "text", text: "{}" }} onChange={() => {}} onModeChange={() => {}} />);
    expect(screen.queryByRole("button", { name: "Prettify JSON" })).toBeNull();
  });

  it("edits through the editor so the change can be undone, and Shift+Alt+F does the same", () => {
    const onChange = vi.fn();
    render(<BodyTab tabKey={key} body={json("[1,2]")} onChange={onChange} onModeChange={() => {}} />);
    const range = { full: true };
    const editor = {
      getValue: () => "[1,2]",
      getModel: () => ({ getFullModelRange: () => range }),
      pushUndoStop: vi.fn(),
      executeEdits: vi.fn(),
    };
    act(() => editorProps!.onReady!(editor as never));
    fireEvent.click(screen.getByRole("button", { name: "Prettify JSON" }));
    expect(editor.executeEdits).toHaveBeenCalledWith("plover.prettify", [{ range, text: "[\n  1,\n  2\n]" }]);
    expect(editor.pushUndoStop).toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
    editor.executeEdits.mockClear();
    act(() => editorProps!.onFormat!());
    expect(editor.executeEdits).toHaveBeenCalledTimes(1);
  });
});
