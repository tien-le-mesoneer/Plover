import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { SendResultDto } from "../../api/types";
import { ResponseViewer } from "../ResponseViewer";
import { suggestedName } from "./ValueMenu";

const focus = vi.fn();
let pick: ((offset: number, at: { x: number; y: number }) => void) | undefined;
let shown = "";

vi.mock("../../editor/CodeEditor", () => ({
  CodeEditor: (p: { value: string; onReady?: (e: unknown) => void; onPick?: typeof pick }) => {
    p.onReady?.({ focus, getAction: () => null });
    pick = p.onPick;
    shown = p.value;
    return <pre data-testid="editor">{p.value}</pre>;
  },
}));

let writeText: ReturnType<typeof vi.fn>;
let setRuntimeVar: ReturnType<typeof vi.fn>;

const doc = { id: "d-8812", signers: [{ email: "a@example.test" }] };

function result(): SendResultDto {
  const bodyText = JSON.stringify(doc);
  return {
    response: {
      status: 201,
      headers: [["content-type", "application/json"]],
      elapsedMs: 1,
      size: bodyText.length,
      sizeComplete: true,
      bodyText,
      bodyBase64: null,
      truncated: false,
    },
    tests: [],
    asserts: [],
    console: [],
    scriptError: null,
    scriptedRequests: [],
    skipped: null,
  };
}

const clickOn = (needle: string) => act(() => pick!(shown.indexOf(needle) + 1, { x: 10, y: 20 }));

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  setRuntimeVar = vi.fn(async () => undefined);
  setApi({ setRuntimeVar } as unknown as ApiClient);
  focus.mockClear();
});

afterEach(cleanup);

describe("value menu", () => {
  it("click_value_opens_menu", () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"d-8812"');
    expect(screen.getByRole("menu", { name: "Value at $.id" })).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Copy value" }));
  });

  it("copy_value_strips_quotes", async () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"d-8812"');
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Copy value" }));
    });
    expect(writeText).toHaveBeenCalledWith("d-8812");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("copy_path_for_nested", async () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"a@example.test"');
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Copy path" }));
    });
    expect(writeText).toHaveBeenCalledWith("$.signers[0].email");
  });

  it("save_as_variable_calls_api_with_prefilled_name", async () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"d-8812"');
    fireEvent.click(screen.getByRole("menuitem", { name: "Save as runtime variable…" }));
    const input = screen.getByRole("textbox", { name: "Runtime variable name" }) as HTMLInputElement;
    expect(input.value).toBe("id");
    fireEvent.change(input, { target: { value: "docId" } });
    fireEvent.click(screen.getByRole("button", { name: "Save variable" }));
    await waitFor(() => expect(setRuntimeVar).toHaveBeenCalledWith("c1", "docId", "d-8812"));
  });

  it("invalid_name_blocks_save", () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"d-8812"');
    fireEvent.click(screen.getByRole("menuitem", { name: "Save as runtime variable…" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Runtime variable name" }), { target: { value: "1bad" } });
    expect((screen.getByRole("button", { name: "Save variable" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Start with a letter/)).toBeTruthy();
  });

  it("esc_returns_focus_to_editor", () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    clickOn('"d-8812"');
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(focus).toHaveBeenCalled();
  });

  it("copy path is off while a filter is active", () => {
    render(<ResponseViewer result={result()} sending={false} collectionId="c1" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Filter with JSONPath" }), { target: { value: "$.signers" } });
    clickOn('"a@example.test"');
    const copyPath = screen.getByRole("menuitem", { name: "Copy path" }) as HTMLButtonElement;
    expect(copyPath.disabled).toBe(true);
    expect(copyPath.title).toBe("Paths are relative to the filtered result");
  });

  it("suggests the last key as the name", () => {
    expect(suggestedName("$.data.id")).toBe("id");
    expect(suggestedName("$.signers[0]")).toBe("signers");
    expect(suggestedName("$['a b']")).toBe("value");
    expect(suggestedName("$")).toBe("value");
  });
});
