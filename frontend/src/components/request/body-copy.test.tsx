import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { BodyDto } from "../../api/types";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { BodyTab } from "./BodyTab";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

let writeText: ReturnType<typeof vi.fn>;
let resolveText: ReturnType<typeof vi.fn>;
let key: string;

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  resolveText = vi.fn(async (_id: string, body: { text: string }) => ({ text: body.text.replace("{{x}}", "1"), containsSecret: false }));
  setApi({ resolveText } as unknown as ApiClient);
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    environments: { c1: [] },
    activeEnv: { c1: "local" },
  });
  useTabs.setState({ tabs: [], active: null, saveDialog: null });
  key = useTabs.getState().newScratch("c1");
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const json: BodyDto = { mode: "json", text: '{"a":"{{x}}"}' };

describe("request body copy", () => {
  it("request_body_copy_copies_resolved_text", async () => {
    render(<BodyTab tabKey={key} body={json} onChange={() => {}} onModeChange={() => {}} />);
    const button = document.querySelector(".editor-box.has-corner .corner-actions button")!;
    expect(button.getAttribute("aria-label")).toBe("Copy body");
    await act(async () => {
      fireEvent.click(button);
    });
    expect(resolveText).toHaveBeenCalledWith("c1", expect.objectContaining({ text: '{"a":"{{x}}"}', env: "local", reveal: false }));
    expect(writeText).toHaveBeenCalledWith('{"a":"1"}');
    expect(screen.getByText("Copied")).toBeTruthy();
  });

  it("request_body_copy_with_secret_asks_first", async () => {
    resolveText.mockImplementation(async (_id: string, body: { reveal: boolean }) =>
      body.reveal ? { text: "secret-body", containsSecret: true } : { text: null, containsSecret: true },
    );
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<BodyTab tabKey={key} body={json} onChange={() => {}} onModeChange={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(confirm).toHaveBeenCalledWith("This value is a secret. Copy anyway?");
    expect(resolveText).toHaveBeenCalledTimes(1);
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.getByText("Not copied")).toBeTruthy();
    confirm.mockReturnValue(true);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy body" }));
    });
    expect(resolveText).toHaveBeenLastCalledWith("c1", expect.objectContaining({ reveal: true }));
    expect(writeText).toHaveBeenCalledWith("secret-body");
  });

  it("no_corner_copy_for_form_body", () => {
    render(
      <BodyTab
        tabKey={key}
        body={{ mode: "formUrlEncoded", fields: [] }}
        onChange={() => {}}
        onModeChange={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "Copy body" })).toBeNull();
  });
});
