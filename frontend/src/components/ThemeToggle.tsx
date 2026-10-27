import type { ReactNode } from "react";
import { nextTheme, type ThemeChoice } from "../lib/appTheme";
import { useLayout } from "../store/layout";
import { MonitorIcon, MoonIcon, SunIcon } from "./icons";

const LABEL: Record<ThemeChoice, string> = { system: "System", light: "Light", dark: "Dark" };
const ICON: Record<ThemeChoice, ReactNode> = { system: <MonitorIcon />, light: <SunIcon />, dark: <MoonIcon /> };

/** Top-bar button: each click moves System → Light → Dark → System. */
export function ThemeToggle() {
  const current = useLayout((s) => s.theme) ?? "system";
  const next = nextTheme(current);
  return (
    <button
      type="button"
      className="icon"
      aria-label={`Theme: ${LABEL[current]}`}
      title={`Theme: ${LABEL[current]}. Click for ${LABEL[next]}`}
      onClick={() => useLayout.getState().setTheme(next)}
    >
      {ICON[current]}
    </button>
  );
}
