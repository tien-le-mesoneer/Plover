import { describe, expect, it } from "vitest";
import type { HttpRequestDto } from "../api/types";
import { switchBodyMode } from "./body";

const draft = (patch: Partial<HttpRequestDto>) => ({ body: { mode: "none" }, otherBodies: [], ...patch }) as HttpRequestDto;

describe("switchBodyMode", () => {
  it("json -> none -> json restores the JSON text", () => {
    const start = draft({ body: { mode: "json", text: '{"a":1}' } });
    const none = switchBodyMode(start, "none");
    expect(none.body).toEqual({ mode: "none" });
    expect(none.otherBodies).toEqual([{ mode: "json", text: '{"a":1}' }]);
    const back = switchBodyMode(none, "json");
    expect(back.body).toEqual({ mode: "json", text: '{"a":1}' });
    expect(back.otherBodies).toEqual([]);
  });

  it("keeps each mode's own content and starts new modes empty", () => {
    let d = draft({ body: { mode: "json", text: "j" }, otherBodies: [{ mode: "xml", text: "<x/>" }] });
    d = switchBodyMode(d, "xml");
    expect(d.body).toEqual({ mode: "xml", text: "<x/>" });
    d = switchBodyMode(d, "formUrlEncoded");
    expect(d.body).toEqual({ mode: "formUrlEncoded", fields: [] });
    expect(d.otherBodies).toEqual([{ mode: "json", text: "j" }, { mode: "xml", text: "<x/>" }]);
  });
});

describe("large bodies", () => {
  it("renderSlice caps at 1 MiB", async () => {
    const { renderSlice, RENDER_CAP } = await import("./body");
    const big = "x".repeat(2 * 1024 * 1024);
    const slice = renderSlice(big);
    expect(slice.capped).toBe(true);
    expect(slice.shown.length).toBe(RENDER_CAP);
    expect(slice.total).toBe(big.length);
    expect(renderSlice("small")).toEqual({ shown: "small", capped: false, total: 5 });
  });

  it("big_arrays_start_folded", async () => {
    const { foldTargets } = await import("./body");
    const text = JSON.stringify({ big: Array.from({ length: 60 }, (_, i) => ({ i })), small: Array.from({ length: 40 }, (_, i) => i) }, null, 2);
    const targets = foldTargets(text);
    expect(targets).toHaveLength(1);
    const lines = text.split("\n");
    expect(lines[targets[0]!.startLine - 1]).toBe('  "big": [');
    expect(lines[targets[0]!.endLine - 1]).toBe("  ],");
  });

  it("counts entries, not commas inside strings", async () => {
    const { foldTargets } = await import("./body");
    const text = JSON.stringify({ s: ",".repeat(100), a: [1, 2] }, null, 2);
    expect(foldTargets(text, 1)).toEqual([
      { startLine: 1, endLine: text.split("\n").length },
      { startLine: 3, endLine: 6 },
    ]);
  });
});
