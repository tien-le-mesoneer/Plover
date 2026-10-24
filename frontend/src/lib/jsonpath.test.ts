import { describe, expect, it } from "vitest";
import { evaluateJsonPath, filterText } from "./jsonpath";

const doc = {
  id: "d-1",
  status: "PENDING",
  signers: [
    { id: "s-1", email: "a@example.test" },
    { id: "s-2", email: "b@example.test" },
    { id: "s-3", email: "c@example.test" },
  ],
  "a b": 1,
  a: { x: 1, y: 2 },
  list: [1, 2, 3],
};

const values = (expr: string) => {
  const r = evaluateJsonPath(doc, expr);
  if (!r.ok) throw new Error(r.error);
  return r.values;
};

describe("JSONPath subset", () => {
  it("$.signers[*].email lists three emails", () => {
    expect(values("$.signers[*].email")).toEqual(["a@example.test", "b@example.test", "c@example.test"]);
  });

  it("..id finds nested ids in document order", () => {
    expect(values("$..id")).toEqual(["d-1", "s-1", "s-2", "s-3"]);
  });

  it("quoted keys, negative indexes and wildcards", () => {
    expect(values("$['a b']")).toEqual([1]);
    expect(values('$["a b"]')).toEqual([1]);
    expect(values("$.list[-1]")).toEqual([3]);
    expect(values("$.list[0]")).toEqual([1]);
    expect(values("$.a.*")).toEqual([1, 2]);
    expect(values("$")).toEqual([doc]);
    expect(values("$.missing")).toEqual([]);
  });

  it("filter text: one value or an array", () => {
    expect(filterText(evaluateJsonPath(doc, "$.id"))).toBe('"d-1"');
    expect(filterText(evaluateJsonPath(doc, "$.list[*]"))).toBe("[\n  1,\n  2,\n  3\n]");
  });

  it("reports unsupported syntax with its position", () => {
    const r = evaluateJsonPath(doc, "$.a[?(@.x)]");
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.at).toBe(3);
      expect(r.error).toMatch(/^Unsupported at 4/);
    }
    expect(evaluateJsonPath(doc, "signers").ok).toBe(false);
    expect(evaluateJsonPath(doc, "$['a").ok).toBe(false);
  });

  it("does not throw on a 200-level nested doc", () => {
    let deep: unknown = { id: "leaf" };
    for (let i = 0; i < 200; i++) deep = { child: deep };
    const r = evaluateJsonPath(deep, "$..id");
    expect(r).toEqual({ ok: true, values: ["leaf"] });
    let deeper: unknown = 1;
    for (let i = 0; i < 600; i++) deeper = [deeper];
    expect(evaluateJsonPath(deeper, "$..*").ok).toBe(false);
  });

  it("filter_2mib_under_budget", () => {
    const rows = Array.from({ length: 45000 }, (_, i) => ({ id: i, name: `row-${i}`, tags: ["a", "b"] }));
    const text = JSON.stringify(rows);
    expect(text.length).toBeGreaterThan(2 * 1024 * 1024);
    const parsed = JSON.parse(text);
    const start = performance.now();
    const r = evaluateJsonPath(parsed, "$[*].id");
    const ms = performance.now() - start;
    expect(r.ok && r.values.length).toBe(45000);
    expect(ms).toBeLessThan(200);
  });
});
