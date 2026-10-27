import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import type { RuntimeVarsDto, VarDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useVars } from "../store/vars";
import { useWorkspace } from "../store/workspace";
import { RuntimeVarsPanel } from "./RuntimeVarsPanel";

/** A fake server: runtime variables of collection `c`; masked names are listed in `masked`. */
let store: Record<string, string>;
let masked: Set<string>;
let runtimeVars: ReturnType<typeof vi.fn>;
let deleteRuntimeVar: ReturnType<typeof vi.fn>;
let clearRuntimeVars: ReturnType<typeof vi.fn>;
let writeText: ReturnType<typeof vi.fn>;

function dto(): RuntimeVarsDto {
  const vars: VarDto[] = Object.keys(store)
    .sort()
    .map((name) => ({
      name,
      source: "runtime",
      value: masked.has(name) ? null : store[name]!,
      masked: masked.has(name),
      truncated: false,
      shadows: [],
    }));
  return { vars };
}

beforeEach(() => {
  store = { docId: "d-1", token: "tok-AAAA" };
  masked = new Set(["token"]);
  runtimeVars = vi.fn(async () => dto());
  deleteRuntimeVar = vi.fn(async (_c: string, name: string) => {
    delete store[name];
  });
  clearRuntimeVars = vi.fn(async () => {
    store = {};
  });
  const setRuntimeVar = vi.fn(async (_c: string, name: string, value: unknown) => {
    store[name] = String(value);
  });
  setApi({
    runtimeVars,
    deleteRuntimeVar,
    clearRuntimeVars,
    setRuntimeVar,
    vars: vi.fn(async () => ({ vars: [], setLater: [] })),
  } as unknown as ApiClient);
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  useWorkspace.setState({ activeEnv: { c: "Dev" } });
  useTabs.setState({ tabs: [], runTabs: [], active: null });
});

afterEach(cleanup);

const openPanel = async () => {
  fireEvent.click(screen.getByRole("button", { name: "Runtime variables" }));
  return await screen.findByRole("dialog", { name: "Runtime variables" });
};

describe("runtime-variable panel", () => {
  it("opens_and_lists_with_masked_values", async () => {
    render(<RuntimeVarsPanel collectionId="c" />);
    const button = screen.getByRole("button", { name: "Runtime variables" });
    expect(button.getAttribute("aria-haspopup")).toBe("dialog");
    const dialog = await openPanel();
    await waitFor(() => expect(dialog.textContent).toContain("docId"));
    expect(runtimeVars).toHaveBeenCalledWith("c", "Dev");
    expect(dialog.textContent).toContain("d-1");
    expect(dialog.textContent).not.toContain("tok-AAAA");
    expect(screen.getByLabelText("masked").textContent).toBe("••••");
    fireEvent.click(screen.getByRole("button", { name: "Copy name docId" }));
    expect(writeText).toHaveBeenCalledWith("docId");
  });

  it("remove_row_calls_delete_and_refreshes_hover", async () => {
    const refreshCollection = vi.spyOn(useVars.getState(), "refreshCollection");
    render(<RuntimeVarsPanel collectionId="c" />);
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Remove docId" }));
    await waitFor(() => expect(deleteRuntimeVar).toHaveBeenCalledWith("c", "docId"));
    await waitFor(() => expect(screen.queryByText("docId")).toBeNull());
    expect(refreshCollection).toHaveBeenCalledWith("c");
  });

  it("clear_all_asks_first", async () => {
    render(<RuntimeVarsPanel collectionId="c" />);
    await openPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Clear all" }));
    expect(screen.getByText("Clear 2 runtime variables?")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(clearRuntimeVars).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(clearRuntimeVars).toHaveBeenCalledWith("c"));
    expect(
      await screen.findByText("No runtime variables. Scripts and Save as variable add them."),
    ).toBeTruthy();
  });

  it("refetches_when_a_send_finishes_while_open", async () => {
    const tab = { key: "t1", collectionId: "c", sending: true } as never;
    useTabs.setState({ tabs: [tab] });
    render(<RuntimeVarsPanel collectionId="c" />);
    await openPanel();
    await waitFor(() => expect(runtimeVars).toHaveBeenCalledTimes(1));
    store.fresh = "new";
    act(() => useTabs.setState({ tabs: [{ ...(tab as object), sending: false } as never] }));
    expect(await screen.findByText("fresh")).toBeTruthy();
  });

  it("escape_closes_and_returns_focus", async () => {
    render(<RuntimeVarsPanel collectionId="c" />);
    const button = screen.getByRole("button", { name: "Runtime variables" });
    const dialog = await openPanel();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it("saved_note_then_panel_shows_var", async () => {
    render(<RuntimeVarsPanel collectionId="c" />);
    const { api } = await import("../api/instance");
    await api().setRuntimeVar("c", "saved", "v-1");
    await openPanel();
    expect(await screen.findByText("saved")).toBeTruthy();
  });
});
