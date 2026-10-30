// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { contrast } from "./lib/contrast";

const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const fontsCss = readFileSync(new URL("./assets/fonts/fonts.css", import.meta.url), "utf8");

/** The light token block (`:root { … }` first in the file) and the dark one (`:root[data-theme="dark"]`). */
function blocks(): { light: string; dark: string; rest: string } {
  const light = /^:root \{([^}]*)\}/m.exec(css);
  const dark = /^:root\[data-theme="dark"\] \{([^}]*)\}/m.exec(css);
  if (!light || !dark) throw new Error("token blocks not found");
  const rest = css.replace(light[0], "").replace(dark[0], "");
  return { light: light[1]!, dark: dark[1]!, rest };
}

function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--([\w-]+):\s*([^;]+);/g)) out[m[1]!] = m[2]!.trim();
  return out;
}

const { light, dark, rest } = blocks();
const themes = { light: tokens(light), dark: tokens(dark) };

const TEXT = ["fg", "muted", "placeholder", "ok", "info", "warn", "err", "get", "post", "put", "patch", "delete", "method-other"];

describe.each(Object.entries(themes))("%s theme", (_name, t) => {
  it("text_tokens_meet_4_5", () => {
    for (const bg of ["bg", "bg-2"]) {
      for (const fg of TEXT) {
        expect(contrast(t[fg]!, t[bg]!), `--${fg} on --${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
    expect(contrast(t["color-on-send"]!, t["color-send"]!)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t["color-on-send"]!, t["color-send-hover"]!)).toBeGreaterThanOrEqual(4.5);
  });

  it("variable_marks_are_visible", () => {
    // `{{names}}` are drawn in --var / --warn / --err text, so they need text contrast (4.5:1).
    for (const bg of ["bg", "bg-2"]) {
      for (const mark of ["var", "warn", "err"]) {
        expect(contrast(t[mark]!, t[bg]!), `--${mark} on --${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("control_borders_meet_3", () => {
    for (const bg of ["bg", "bg-2"]) {
      expect(contrast(t["border-strong"]!, t[bg]!), `--border-strong on --${bg}`).toBeGreaterThanOrEqual(3);
      expect(contrast(t["focus"]!, t[bg]!), `--focus on --${bg}`).toBeGreaterThanOrEqual(3);
    }
  });

  it("primary_and_danger_buttons_meet_4_5", () => {
    // .primary: --bg on --fg, --bg on --muted when hovered; .danger: --err on --bg-2, --bg on --err when hovered.
    expect(contrast(t["bg"]!, t["fg"]!)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t["bg"]!, t["muted"]!)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t["err"]!, t["bg-2"]!)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t["bg"]!, t["err"]!)).toBeGreaterThanOrEqual(4.5);
  });

  it("send differs from GET and 2xx", () => {
    expect(t["color-send"]).not.toBe(t["get"]);
    expect(t["color-send"]).not.toBe(t["ok"]);
  });
});

it("send_is_emerald_700", () => {
  expect(themes.light["color-send"]).toBe("#047857");
  expect(themes.light["color-on-send"]).toBe("#ffffff");
});

/** The declarations of the first rule whose selector list is exactly `selector`. */
const rule = (selector: string) => {
  const at = rest.indexOf(`\n${selector} {`);
  if (at < 0) throw new Error(`${selector} not found`);
  return rest.slice(at, rest.indexOf("}", at));
};

it("primary_and_danger_use_the_checked_pairs", () => {
  expect(rule("button.primary")).toMatch(/background: var\(--fg\);.*color: var\(--bg\);/s);
  expect(rule("button.primary:hover:not(:disabled)")).toMatch(/background: var\(--muted\);/);
  expect(rule("button.danger")).toMatch(/border-color: var\(--err\);.*color: var\(--err\);/s);
  expect(rule("button.danger:hover:not(:disabled)")).toMatch(/background: var\(--err\);.*color: var\(--bg\);/s);
});

it("toolbar_controls_are_30px", () => {
  // Text and icon buttons in a toolbar row are as tall as its inputs and selects.
  expect(themes.light["control-h"]).toBe("30px");
  expect(rule(".topbar button, .urlbar button, .run-header button")).toMatch(/min-height: var\(--control-h\);/);
  expect(rule(".topbar button.icon, .urlbar button.icon, .run-header button.icon")).toMatch(/min-width: var\(--control-h\);/);
});

it("one_selected_row_style", () => {
  // Sidebar, run list, history, quick open and the variable list mark their row the same way.
  expect(rule(".is-selected")).toMatch(/background: var\(--selected\);.*box-shadow: inset 2px 0 0 var\(--focus\);/s);
  expect(rest).not.toMatch(/\.selected\b|li\.active\b|\[aria-selected="true"\] \{/);
  expect(rest).not.toMatch(/font-weight[^;]*;[^}]*inset \d+px 0/);
});

it("hover_uses_one_token", () => {
  // --hover is --bg-2 on --bg surfaces and --bg-3 on --bg-2 surfaces (top bar, sidebar).
  expect(themes.light["hover"]).toBe("var(--bg-2)");
  expect(rule(".topbar, .sidebar, .tabbar")).toMatch(/--hover: var\(--bg-3\);/);
  const hovers = [...rest.matchAll(/([^{}]*:hover[^{}]*)\{([^}]*)\}/g)].filter((m) => /(^|[\s;])background: var\(--bg-[23]\)/.test(m[2]!));
  expect(hovers.map((m) => m[1]!.trim())).toEqual([]);
});

it("one_floating_surface", () => {
  // Menus, popovers, the variable card and the name list share one look and one shadow.
  expect(rule(".surface")).toMatch(
    /background: var\(--bg\);.*border: 1px solid var\(--border-strong\);.*border-radius: var\(--radius\);.*padding: 4px;.*box-shadow: var\(--shadow\);/s,
  );
  expect(rule(".menu-item:hover:not(:disabled), .menu-item:focus-visible")).toMatch(/background: var\(--hover\);/);
  // A surface is drawn in --bg, even inside the --bg-2 top bar.
  expect(rule(".surface")).toMatch(/--hover: var\(--bg-2\);/);
  const shadows = [...rest.matchAll(/box-shadow: ([^;]+);/g)].map((m) => m[1]!).filter((v) => !v.startsWith("inset"));
  expect(new Set(shadows)).toEqual(new Set(["var(--shadow)"]));
  // The superseded per-menu looks are gone.
  for (const sel of [".context-menu", ".value-menu", ".menu-button .menu", ".export-menu .menu", ".popover"]) {
    const own = rest.match(new RegExp(`\\n${sel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\{([^}]*)\\}`))?.[1] ?? "";
    expect(own, sel).not.toMatch(/background:|border:|box-shadow:/);
  }
});

it("one_table_style", () => {
  // Column headers of every table (key-value, response headers, HAR) look the same.
  expect(rule("thead th")).toMatch(/text-align: left;.*font-size: 12px;.*font-weight: 500;.*color: var\(--muted\);/s);
  const others = [...rest.matchAll(/\n([^{}\n]*thead th[^{}]*)\{([^}]*)\}/g)].filter((m) => m[1]!.trim() !== "thead th");
  for (const m of others) expect(m[2], m[1]).not.toMatch(/font-(size|weight|family)|(^|[\s;])color:/);
  expect(rule(".kv th")).not.toMatch(/font-|color:/);
  // Read-only tables separate their rows.
  expect(rule("table.headers tbody th, table.headers tbody td")).toMatch(/border-bottom: 1px solid var\(--border\);/);
  expect(rule(".har-table th, .har-table td")).toMatch(/border-bottom: 1px solid var\(--border\);/);
});

it("key_value_table_starts_at_the_pane_edge", () => {
  // The checkbox column is as wide as the checkbox, flush with the h4 and select above.
  // `.kv` in front: it has to win over `.kv td { padding: 2px }`.
  expect(rule(".kv .kv-check")).toMatch(/padding-left: 0;.*text-align: left;/s);
  expect(rule(".kv .kv-check input")).toMatch(/margin: 0;/);
});

it("no_raw_colours_outside_tokens", () => {
  const withoutComments = rest.replace(/\/\*[\s\S]*?\*\//g, "");
  expect(withoutComments).not.toMatch(/#[0-9a-f]{3,8}\b/i);
  expect(withoutComments).not.toMatch(/\brgba?\(/i);
});

it("fonts_are_local", () => {
  const faces = [...fontsCss.matchAll(/@font-face \{([^}]*)\}/g)].map((m) => m[1]!);
  expect(faces.length).toBeGreaterThanOrEqual(10);
  for (const face of faces) {
    expect(face).toMatch(/src: url\(\.\/[\w-]+\.woff2\)/);
    expect(face).toMatch(/font-display: swap/);
  }
  expect(fontsCss).not.toMatch(/https?:/);
});

it("font tokens name the bundled fonts", () => {
  expect(themes.light["font-ui"]).toMatch(/^"IBM Plex Sans"/);
  expect(themes.light["font-mono"]).toMatch(/^"JetBrains Mono"/);
});

describe("hover-revealed controls", () => {
  it("css_reveals_on_hover_focus_and_no_hover", () => {
    for (const sel of [".collection-header .icon", ".corner-actions"]) {
      expect(css).toContain(sel);
    }
    expect(css).toMatch(/\.collection-header:hover \.icon/);
    expect(css).toMatch(/\.collection-header:focus-within \.icon/);
    expect(css).toMatch(/\.has-corner:hover \.corner-actions/);
    expect(css).toMatch(/\.has-corner:focus-within \.corner-actions/);
    const noHover = css.match(/@media \(hover: none\) \{([^}]*)\}/)?.[1] ?? "";
    expect(noHover).toContain(".collection-header .icon");
    expect(noHover).toContain(".corner-actions");
    // Hidden with opacity, never removed from the Tab order.
    expect(css).not.toMatch(/\.collection-header \.icon\s*\{[^}]*display:\s*none/);
  });
});

describe("no clash with Monaco's class names", () => {
  it("app rules do not use bare class names Monaco widgets use", () => {
    // Monaco's suggest rows contain .main, .left, .right, .contents and .details.
    for (const name of ["main", "left", "right", "contents", "details"]) {
      expect(css).not.toMatch(new RegExp(`(^|[\\s,}])\\.${name}\\s*[{,]`, "m"));
    }
  });
});

describe("selected row colours", () => {
  it("every text token and the focus bar stay readable on a selected row", () => {
    // Status, method and variable colours sit on it (history, run results, sidebar); --bg-3 is too dark for --ok.
    for (const [name, t] of Object.entries(themes)) {
      for (const fg of [...TEXT, "var"]) {
        expect(contrast(t[fg]!, t.selected!), `${name}: --${fg} on --selected`).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast(t.focus!, t.selected!), `${name}: --focus on --selected`).toBeGreaterThanOrEqual(3);
    }
  });
});


it("one_dark_block_keyed_on_data_theme", () => {
  // The OS scheme reaches the page only through data-theme (theme-boot.js, lib/appTheme.ts).
  expect(css).not.toMatch(/prefers-color-scheme/);
  expect(css.match(/data-theme="dark"/g)?.length).toBe(1);
});
