import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTheme, onResolvedTheme, resolvedTheme, setThemeChoice, startTheme } from "./appTheme";

function fakeQuery(matches: boolean) {
  let listener: ((e: { matches: boolean }) => void) | undefined;
  return {
    matches,
    addEventListener: (_: "change", l: (e: { matches: boolean }) => void) => (listener = l),
    removeEventListener: vi.fn(),
    flip(to: boolean) {
      this.matches = to;
      listener?.({ matches: to });
    },
  };
}

let stop: (() => void) | undefined;
afterEach(() => {
  stop?.();
  stop = undefined;
});

describe("app theme", () => {
  it("cycles system, light, dark", () => {
    expect(nextTheme("system")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("system");
  });

  it("system follows the OS scheme live and sets data-theme", () => {
    const root = document.createElement("html");
    const q = fakeQuery(true);
    stop = startTheme(q, root);
    expect(root.dataset.theme).toBe("dark");
    expect(resolvedTheme()).toBe("dark");
    q.flip(false);
    expect(root.dataset.theme).toBe("light");
  });

  it("an explicit choice ignores OS changes until set back to system", () => {
    const root = document.createElement("html");
    const q = fakeQuery(false);
    stop = startTheme(q, root);
    setThemeChoice("dark");
    expect(root.dataset.theme).toBe("dark");
    q.flip(false);
    expect(root.dataset.theme).toBe("dark");
    setThemeChoice("light");
    q.flip(true);
    expect(root.dataset.theme).toBe("light");
    setThemeChoice("system");
    expect(root.dataset.theme).toBe("dark");
  });

  it("tells listeners the resolved theme, only when it changes", () => {
    const root = document.createElement("html");
    const q = fakeQuery(false);
    stop = startTheme(q, root);
    const seen: string[] = [];
    const off = onResolvedTheme((t) => seen.push(t));
    setThemeChoice("light");
    setThemeChoice("dark");
    q.flip(true);
    setThemeChoice("system");
    off();
    setThemeChoice("light");
    expect(seen).toEqual(["dark"]);
  });

  it("stop removes the OS listener", () => {
    const q = fakeQuery(false);
    startTheme(q, document.createElement("html"))();
    expect(q.removeEventListener).toHaveBeenCalled();
  });
});
