import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import { effectiveSidebarWidth, useLayout, useSidebarWidth } from "../store/layout";
import { SidebarResizer } from "./SidebarResizer";

let saveLayout: ReturnType<typeof vi.fn>;

function setWindowWidth(px: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: px });
  window.dispatchEvent(new Event("resize"));
}

/** The sidebar width and its handle, as App renders them. */
function Harness() {
  const width = useSidebarWidth();
  return (
    <>
      <div data-testid="sidebar" style={{ width }} />
      <SidebarResizer width={width} />
    </>
  );
}

const separator = () => screen.getByRole("separator", { name: "Resize sidebar" });
const width = () => Number(separator().getAttribute("aria-valuenow"));

beforeEach(() => {
  vi.useFakeTimers();
  saveLayout = vi.fn(async () => undefined);
  setApi({ saveLayout, layout: vi.fn(async () => ({ sidebarWidth: 320 })) } as unknown as ApiClient);
  useLayout.setState({ sidebarWidth: null });
  setWindowWidth(1000);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("resizable sidebar", () => {
  it("drag_resizes_within_bounds", () => {
    render(<Harness />);
    expect(width()).toBe(260);
    fireEvent.pointerDown(separator(), { clientX: 260, pointerId: 1 });
    fireEvent.pointerMove(separator(), { clientX: 100, pointerId: 1 });
    expect(width()).toBe(180);
    fireEvent.pointerMove(separator(), { clientX: 2000, pointerId: 1 });
    expect(width()).toBe(500);
    fireEvent.pointerUp(separator(), { pointerId: 1 });
    act(() => vi.advanceTimersByTime(400));
    expect(saveLayout).toHaveBeenLastCalledWith({ sidebarWidth: 500 });
  });

  it("keys_resize_by_16", () => {
    render(<Harness />);
    fireEvent.keyDown(separator(), { key: "ArrowRight" });
    expect(width()).toBe(276);
    fireEvent.keyDown(separator(), { key: "ArrowLeft" });
    fireEvent.keyDown(separator(), { key: "ArrowLeft" });
    expect(width()).toBe(244);
    fireEvent.keyDown(separator(), { key: "Home" });
    expect(width()).toBe(180);
    fireEvent.keyDown(separator(), { key: "End" });
    expect(width()).toBe(500);
  });

  it("double_click_resets", () => {
    useLayout.setState({ sidebarWidth: 400 });
    render(<Harness />);
    expect(width()).toBe(400);
    fireEvent.doubleClick(separator());
    expect(width()).toBe(260);
    expect(useLayout.getState().sidebarWidth).toBeNull();
  });

  it("width_survives_reload", async () => {
    await useLayout.getState().load();
    render(<Harness />);
    expect(width()).toBe(320);
  });

  it("stored_width_clamps_to_window", () => {
    setWindowWidth(1280);
    useLayout.setState({ sidebarWidth: 1200 });
    render(<Harness />);
    expect(width()).toBe(640);
    act(() => setWindowWidth(800));
    expect(width()).toBe(400);
    act(() => vi.advanceTimersByTime(1000));
    expect(saveLayout).not.toHaveBeenCalled();
    expect(useLayout.getState().sidebarWidth).toBe(1200);
  });

  it("effective width keeps the minimum in a tiny window", () => {
    expect(effectiveSidebarWidth(null, 300)).toBe(180);
  });
});
