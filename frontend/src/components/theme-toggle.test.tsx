import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import { startTheme } from "../lib/appTheme";
import { useLayout } from "../store/layout";
import { ThemeToggle } from "./ThemeToggle";

let saveLayout: ReturnType<typeof vi.fn>;
let stop: () => void;

beforeEach(() => {
  vi.useFakeTimers();
  saveLayout = vi.fn(async () => undefined);
  setApi({ saveLayout } as unknown as ApiClient);
  useLayout.setState({ sidebarWidth: null, splitOrientation: null, splitPermille: null, stackedSplitPermille: null, theme: null });
  stop = startTheme(
    { matches: false, addEventListener: () => {}, removeEventListener: () => {} },
    document.documentElement,
  );
});

afterEach(() => {
  stop();
  cleanup();
  vi.useRealTimers();
});

describe("theme toggle", () => {
  it("names the current choice and says what the next click does", () => {
    render(<ThemeToggle />);
    const button = screen.getByRole("button", { name: "Theme: System" });
    expect(button.getAttribute("title")).toBe("Theme: System. Click for Light");
  });

  it("cycles System → Light → Dark → System, applies it and saves it", () => {
    render(<ThemeToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Theme: System" }));
    expect(document.documentElement.dataset.theme).toBe("light");
    fireEvent.click(screen.getByRole("button", { name: "Theme: Light" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByRole("button", { name: "Theme: Dark" }).getAttribute("title")).toBe(
      "Theme: Dark. Click for System",
    );
    act(() => vi.advanceTimersByTime(400));
    expect(saveLayout).toHaveBeenLastCalledWith({
      sidebarWidth: null,
      splitOrientation: null,
      splitPermille: null,
      stackedSplitPermille: null,
      theme: "dark",
    });
    fireEvent.click(screen.getByRole("button", { name: "Theme: Dark" }));
    expect(screen.getByRole("button", { name: "Theme: System" })).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("a saved choice is applied when the layout loads", async () => {
    setApi({ saveLayout, layout: vi.fn(async () => ({ sidebarWidth: null, theme: "dark" })) } as unknown as ApiClient);
    render(<ThemeToggle />);
    await act(async () => {
      await useLayout.getState().load();
    });
    expect(screen.getByRole("button", { name: "Theme: Dark" })).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
