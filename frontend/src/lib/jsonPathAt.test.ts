import { describe, expect, it } from "vitest";
import { jsonPathAt } from "./jsonPathAt";

const doc = { id: "d-8812", signers: [{ email: "a@example.test" }, { email: "b@example.test" }], "a b": 1, n: 3 };
const text = JSON.stringify(doc, null, 2);
const at = (needle: string, delta = 1) => text.indexOf(needle) + delta;

describe("jsonPathAt", () => {
  it("finds a top-level value", () => {
    expect(jsonPathAt(text, at('"d-8812"'))).toMatchObject({ path: "$.id", value: "d-8812" });
    expect(jsonPathAt(text, at("3\n", 0))).toMatchObject({ path: "$.n", value: 3 });
  });

  it("finds a nested value in an array", () => {
    expect(jsonPathAt(text, at('"b@example.test"'))).toMatchObject({ path: "$.signers[1].email", value: "b@example.test" });
  });

  it("quotes keys that are not identifiers", () => {
    expect(jsonPathAt(text, at("1,", 0))).toMatchObject({ path: "$['a b']", value: 1 });
  });

  it("a key gives the value after it", () => {
    expect(jsonPathAt(text, at('"signers"'))).toMatchObject({ path: "$.signers", value: doc.signers });
  });

  it("a bracket gives its container", () => {
    expect(jsonPathAt(text, 0)).toMatchObject({ path: "$", value: doc });
    expect(jsonPathAt(text, text.indexOf("{", 2))).toMatchObject({ path: "$.signers[0]" });
  });

  it("escaped quotes in strings do not break the scan", () => {
    const t = JSON.stringify({ q: 'say "hi" \\ there', after: true }, null, 2);
    expect(jsonPathAt(t, t.indexOf("true"))).toMatchObject({ path: "$.after", value: true });
  });

  it("whitespace between members and non-JSON give null", () => {
    expect(jsonPathAt(text, text.indexOf("\n") + 1)).toBeNull();
    expect(jsonPathAt("<html>", 2)).toBeNull();
  });

  it("jsonPathAt_is_linear_on_deep_input", () => {
    let deep: unknown = { leaf: "x" };
    for (let i = 0; i < 200; i++) deep = { child: deep, pad: "p".repeat(5000) };
    const big = JSON.stringify(deep, null, 2);
    expect(big.length).toBeGreaterThan(1024 * 1024);
    const start = performance.now();
    const found = jsonPathAt(big, big.indexOf('"x"') + 1);
    expect(performance.now() - start).toBeLessThan(100);
    expect(found?.path).toBe("$" + ".child".repeat(200) + ".leaf");
  });
});
