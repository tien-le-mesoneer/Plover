// @vitest-environment node
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it } from "vitest";

const script = fileURLToPath(new URL("./check-no-cdn.mjs", import.meta.url));
let dir;

afterEach(() => {
  if (dir) rmSync(dir, { recursive: true, force: true });
});

function check(contents) {
  dir = mkdtempSync(join(tmpdir(), "plover-cdn-"));
  writeFileSync(join(dir, "index.css"), contents);
  try {
    execFileSync(process.execPath, [script, dir], { stdio: "pipe" });
    return 0;
  } catch (e) {
    return e.status;
  }
}

it("fails on a Google Fonts reference", () => {
  expect(check('@import url("https://fonts.googleapis.com/css2?family=Inter");')).toBe(1);
  expect(check("src: url(https://fonts.gstatic.com/s/x.woff2)")).toBe(1);
});

it("passes on local font files", () => {
  expect(check("src: url(./ibm-plex-sans-latin-400-normal.woff2)")).toBe(0);
});
