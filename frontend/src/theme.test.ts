// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { contrast } from "./lib/contrast";

const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const fontsCss = readFileSync(new URL("./assets/fonts/fonts.css", import.meta.url), "utf8");

/** The light token block (`:root { … }` first in the file) and the dark one (inside the media query). */
function blocks(): { light: string; dark: string; rest: string } {
  const light = /^:root \{([^}]*)\}/m.exec(css);
  const dark = /@media \(prefers-color-scheme: dark\) \{\s*:root \{([^}]*)\}\s*\}/m.exec(css);
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

  it("control_borders_meet_3", () => {
    for (const bg of ["bg", "bg-2"]) {
      expect(contrast(t["border-strong"]!, t[bg]!), `--border-strong on --${bg}`).toBeGreaterThanOrEqual(3);
      expect(contrast(t["focus"]!, t[bg]!), `--focus on --${bg}`).toBeGreaterThanOrEqual(3);
    }
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
