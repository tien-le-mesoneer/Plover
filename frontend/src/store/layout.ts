import { useEffect, useState } from "react";
import { create } from "zustand";
import { api } from "../api/instance";

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 180;
const SAVE_DELAY_MS = 300;

/** The width the sidebar is drawn at: the stored one (or the default), kept between 180 px and half the window. */
export function effectiveSidebarWidth(stored: number | null, windowWidth: number): number {
  const max = Math.max(SIDEBAR_MIN, Math.floor(windowWidth / 2));
  return Math.min(max, Math.max(SIDEBAR_MIN, stored ?? SIDEBAR_DEFAULT));
}

export interface LayoutStore {
  /** The width the user dragged to; `null` is the default. Not rewritten by window resizes. */
  sidebarWidth: number | null;
  load(): Promise<void>;
  /** Sets the width and saves it shortly after the last change. */
  setSidebarWidth(px: number | null): void;
}

let timer: ReturnType<typeof setTimeout> | undefined;

export const useLayout = create<LayoutStore>()((set) => ({
  sidebarWidth: null,
  async load() {
    try {
      const layout = await api().layout();
      set({ sidebarWidth: layout.sidebarWidth });
    } catch {
      // The default width is fine when the server cannot say.
    }
  },
  setSidebarWidth(px) {
    const width = px === null ? null : Math.round(px);
    set({ sidebarWidth: width });
    clearTimeout(timer);
    timer = setTimeout(() => {
      void api()
        .saveLayout({ sidebarWidth: width })
        .catch(() => undefined);
    }, SAVE_DELAY_MS);
  },
}));

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
