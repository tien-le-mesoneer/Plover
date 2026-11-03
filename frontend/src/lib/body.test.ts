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
    const { foldTargets, jsonContainers } = await import("./body");
    const text = JSON.stringify({ big: Array.from({ length: 60 }, (_, i) => ({ i })), small: Array.from({ length: 40 }, (_, i) => i) }, null, 2);
    const targets = foldTargets(jsonContainers(text));
    expect(targets).toHaveLength(1);
    const lines = text.split("\n");
    expect(lines[targets[0]!.startLine - 1]).toBe('  "big": [');
    expect(lines[targets[0]!.endLine - 1]).toBe("  ],");
  });

  it("counts entries, not commas inside strings", async () => {
    const { foldTargets, jsonContainers } = await import("./body");
    const text = JSON.stringify({ s: ",".repeat(100), a: [1, 2] }, null, 2);
    expect(foldTargets(jsonContainers(text), 1)).toEqual([
      { startLine: 1, endLine: text.split("\n").length },
      { startLine: 3, endLine: 6 },
    ]);
  });

  it("counts the direct entries of every array and object", async () => {
    const { jsonContainers } = await import("./body");
    const text = JSON.stringify({ a: [[1, 2], [3], []], o: { x: 1, y: { z: 2 } } }, null, 2);
    const at = (line: string) => {
      const n = text.split("\n").findIndex((l) => l.trimStart().startsWith(line)) + 1;
      return jsonContainers(text).find((c) => c.startLine === n);
    };
    expect(at('"a"')).toMatchObject({ kind: "array", count: 3 });
    expect(at('"o"')).toMatchObject({ kind: "object", count: 2 });
    expect(at('"y"')).toMatchObject({ kind: "object", count: 1 });
    expect(jsonContainers(text).find((c) => c.startLine === 1)).toMatchObject({ kind: "object", count: 2 });
    // `[]` is one entry of `a` and has none itself.
    expect(jsonContainers(text).filter((c) => c.count === 0)).toHaveLength(1);
  });

  it("skips brackets, commas and {{variables}} inside strings", async () => {
    const { jsonContainers } = await import("./body");
    const text = JSON.stringify(["[a, b]", "{{host}}/x,y", 'q\\"],{', { "k[,]": "}" }], null, 2);
    const outer = jsonContainers(text).find((c) => c.startLine === 1);
    expect(outer).toMatchObject({ kind: "array", count: 4 });
    expect(jsonContainers(text).filter((c) => c.kind === "object")).toEqual([
      expect.objectContaining({ count: 1 }),
    ]);
  });

  it("gives no counts for broken JSON and drops containers cut off at the end", async () => {
    const { jsonContainers } = await import("./body");
    expect(jsonContainers("[1, 2}")).toEqual([]);
    expect(jsonContainers("]")).toEqual([]);
    expect(jsonContainers("not json")).toEqual([]);
    expect(jsonContainers('[\n  [1, 2],\n  [3')).toEqual([{ kind: "array", startLine: 2, endLine: 2, count: 2 }]);
  });

  it("labels a count with its noun", async () => {
    const { countLabel } = await import("./body");
    expect(countLabel({ kind: "array", count: 12 })).toBe("12 items");
    expect(countLabel({ kind: "array", count: 1 })).toBe("1 item");
    expect(countLabel({ kind: "object", count: 0 })).toBe("0 keys");
    expect(countLabel({ kind: "object", count: 1 })).toBe("1 key");
  });
});
