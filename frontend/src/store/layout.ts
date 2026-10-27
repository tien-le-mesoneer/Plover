import { useEffect, useState, type RefObject } from "react";
import { create } from "zustand";
import { api } from "../api/instance";
import type { LayoutDto } from "../api/types";

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 180;
const SAVE_DELAY_MS = 300;

/** Request pane share, in thousandths of the split area (`null` = half). */
export const SPLIT_MIN = 200;
export const SPLIT_MAX = 800;
export const SPLIT_DEFAULT = 500;
/** Side by side needs two 240 px panes and the 6 px handle. */
export const SIDE_BY_SIDE_MIN = 2 * 240 + 6;

export type Orientation = "side" | "stacked";

/** The width the sidebar is drawn at: the stored one (or the default), kept between 180 px and half the window. */
export function effectiveSidebarWidth(stored: number | null, windowWidth: number): number {
  const max = Math.max(SIDEBAR_MIN, Math.floor(windowWidth / 2));
  return Math.min(max, Math.max(SIDEBAR_MIN, stored ?? SIDEBAR_DEFAULT));
}

export const clampSplit = (v: number) => Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, Math.round(v)));

export interface LayoutStore {
  /** The width the user dragged to; `null` is the default. Not rewritten by window resizes. */
  sidebarWidth: number | null;
  /** The chosen pane placement; `null` is side by side. */
  splitOrientation: Orientation | null;
  /** The request pane's share side by side; `null` is half. */
  splitPermille: number | null;
  /** The request pane's share when stacked; `null` is half. */
  stackedSplitPermille: number | null;
  load(): Promise<void>;
  /** Sets the width and saves it shortly after the last change. */
  setSidebarWidth(px: number | null): void;
  setSplitOrientation(o: Orientation): void;
  /** Sets one orientation's ratio (`null` resets it), clamped to 200..=800. */
  setSplitPermille(orientation: Orientation, v: number | null): void;
}

let timer: ReturnType<typeof setTimeout> | undefined;

export const useLayout = create<LayoutStore>()((set, get) => {
  // One PUT carries every field, so a later save never drops another one.
  const save = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const { sidebarWidth, splitOrientation, splitPermille, stackedSplitPermille } = get();
      const layout: LayoutDto = { sidebarWidth, splitOrientation, splitPermille, stackedSplitPermille };
      void api()
        .saveLayout(layout)
        .catch(() => undefined);
    }, SAVE_DELAY_MS);
  };
  return {
    sidebarWidth: null,
    splitOrientation: null,
    splitPermille: null,
    stackedSplitPermille: null,
    async load() {
      try {
        const layout = await api().layout();
        set({
          sidebarWidth: layout.sidebarWidth,
          splitOrientation: layout.splitOrientation ?? null,
          splitPermille: layout.splitPermille ?? null,
          stackedSplitPermille: layout.stackedSplitPermille ?? null,
        });
      } catch {
        // The defaults are fine when the server cannot say.
      }
    },
    setSidebarWidth(px) {
      set({ sidebarWidth: px === null ? null : Math.round(px) });
      save();
    },
    setSplitOrientation(splitOrientation) {
      set({ splitOrientation });
      save();
    },
    setSplitPermille(orientation, v) {
      const value = v === null ? null : clampSplit(v);
      set(orientation === "side" ? { splitPermille: value } : { stackedSplitPermille: value });
      save();
    },
  };
});

/** The sidebar width to draw now; follows window resizes without saving anything. */
export function useSidebarWidth(): number {
  const stored = useLayout((s) => s.sidebarWidth);
  const [windowWidth, setWindowWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    const onResize = () => setWindowWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return effectiveSidebarWidth(stored, windowWidth);
}

/**
 * The placement to draw: the saved choice, or stacked while the split area is too narrow
 * for two side-by-side panes (`forced`). Nothing is saved when it is forced.
 */
export function useEffectiveOrientation(splitRef: RefObject<HTMLElement | null>): {
  orientation: Orientation;
  forced: boolean;
} {
  const chosen = useLayout((s) => s.splitOrientation) ?? "side";
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const el = splitRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[entries.length - 1]?.contentRect.width;
      if (width !== undefined) setNarrow(width < SIDE_BY_SIDE_MIN);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [splitRef]);
  return narrow ? { orientation: "stacked", forced: true } : { orientation: chosen, forced: false };
}
