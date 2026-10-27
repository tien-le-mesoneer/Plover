import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import { useLayout } from "../../store/layout";
import { useTabs } from "../../store/tabs";
import { useWorkspace } from "../../store/workspace";
import { RequestEditor } from "./RequestEditor";

vi.mock("../../editor/CodeEditor", () => ({ CodeEditor: () => null }));

/** A fake ResizeObserver that reports a chosen width for every observed element. */
let observers: { cb: ResizeObserverCallback; el: Element }[] = [];
class FakeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(el: Element) {
    observers.push({ cb: this.cb, el });
  }
  unobserve() {}
  disconnect() {
    observers = observers.filter((o) => o.cb !== this.cb);
  }
}
function reportWidth(px: number) {
  act(() => {
    for (const o of observers) o.cb([{ target: o.el, contentRect: { width: px } } as unknown as ResizeObserverEntry], {} as ResizeObserver);
  });
}

let saveLayout: ReturnType<typeof vi.fn>;
let stored: Record<string, unknown>;

function Editor() {
  const tab = useTabs((s) => s.tabs[0]);
  return tab ? <RequestEditor tab={tab} /> : null;
}

const split = () => document.querySelector(".split")!;
const toggleButton = () => screen.getByRole("button", { name: /request and response/ });

beforeEach(() => {
  vi.useFakeTimers();
  observers = [];
  vi.stubGlobal("ResizeObserver", FakeObserver);
  stored = { sidebarWidth: null, splitOrientation: null, splitPermille: null, stackedSplitPermille: null };
  saveLayout = vi.fn(async (l: Record<string, unknown>) => {
    stored = { ...l };
  });
  setApi({ saveLayout, layout: vi.fn(async () => stored), vars: vi.fn(async () => ({ vars: [], setLater: [] })) } as unknown as ApiClient);
  useLayout.setState({ sidebarWidth: null, splitOrientation: null, splitPermille: null, stackedSplitPermille: null });
  useWorkspace.setState({
    collections: [{ id: "c1", name: "Demo", root: "/x", available: true, error: null }],
    environments: { c1: [] },
    activeEnv: { c1: null },
  });
  useTabs.setState({ tabs: [], active: null, saveDialog: null });
  useTabs.getState().newScratch("c1");
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("side by side / stacked toggle", () => {
  it("split_sets_pane_flex_basis", () => {
    useLayout.setState({ splitPermille: 300, stackedSplitPermille: 700 });
    render(<Editor />);
    const request = split().querySelector<HTMLElement>(".pane")!;
    expect(request.style.flexBasis).toBe("30%");
    act(() => useLayout.getState().setSplitOrientation("stacked"));
    expect(split().querySelector<HTMLElement>(".pane")!.style.flexBasis).toBe("70%");
  });

  it("toggle_flips_layout_and_persists", async () => {
    render(<Editor />);
    expect(split().classList.contains("split-side")).toBe(true);
    expect(toggleButton().getAttribute("aria-pressed")).toBe("false");
    expect(toggleButton().getAttribute("aria-label")).toBe("Stack request and response");
    fireEvent.click(toggleButton());
    expect(split().classList.contains("split-stacked")).toBe(true);
    expect(toggleButton().getAttribute("aria-pressed")).toBe("true");
    expect(toggleButton().getAttribute("aria-label")).toBe("Place request and response side by side");
    act(() => vi.advanceTimersByTime(400));
    expect(saveLayout).toHaveBeenCalledTimes(1);
    // A reload: a fresh store loads the layout from the server.
    cleanup();
    useLayout.setState({ splitOrientation: null });
    await act(async () => {
      await useLayout.getState().load();
    });
    render(<Editor />);
    expect(split().classList.contains("split-stacked")).toBe(true);
  });

  it("toggle_keyboard_activation", () => {
    render(<Editor />);
    toggleButton().focus();
    fireEvent.keyDown(toggleButton(), { key: "Enter" });
    expect(useLayout.getState().splitOrientation).toBe("stacked");
    fireEvent.keyDown(toggleButton(), { key: " " });
    expect(useLayout.getState().splitOrientation).toBe("side");
  });

  it("narrow_split_forces_stacked_without_saving", () => {
    render(<Editor />);
    reportWidth(400);
    expect(split().classList.contains("split-stacked")).toBe(true);
    expect(toggleButton().getAttribute("aria-disabled")).toBe("true");
    expect(toggleButton().getAttribute("aria-pressed")).toBe("true");
    expect(toggleButton().getAttribute("title")).toBe("Too narrow to place side by side");
    fireEvent.click(toggleButton());
    act(() => vi.advanceTimersByTime(400));
    expect(saveLayout).not.toHaveBeenCalled();
    expect(useLayout.getState().splitOrientation).toBeNull();
    reportWidth(900);
    expect(split().classList.contains("split-side")).toBe(true);
    expect(toggleButton().getAttribute("aria-disabled")).toBeNull();
  });

  it("toggle_has_label_and_24px_target", () => {
    render(<Editor />);
    expect(toggleButton().getAttribute("aria-label")).toBeTruthy();
    expect(toggleButton().classList.contains("icon")).toBe(true);
    const css = readFileSync("src/styles.css", "utf8");
    expect(css).toMatch(/button\.icon\s*\{[^}]*min-width:\s*24px/);
    expect(css).toMatch(/button\.icon\s*\{[^}]*min-height:\s*24px/);
  });
});
