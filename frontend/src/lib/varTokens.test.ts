import { describe, expect, it } from "vitest";
import type { VarsDto } from "../api/types";
import { classify, explain, tokenize } from "./varTokens";

const dto: VarsDto = {
  vars: [
    { name: "a", source: "environment", value: "{\"b\":[1]}", masked: false, truncated: false, shadows: ["collection"] },
    { name: "apiKey", source: "environment", value: null, masked: true, truncated: false, shadows: [] },
    { name: "process.env.HOME", source: "processEnv", value: null, masked: true, truncated: false, shadows: [] },
  ],
  setLater: [{ name: "token", by: "auth/login.bru" }],
};

describe("tokenize", () => {
  it("finds placeholders like core", () => {
    expect(tokenize("{{a}}/x/{{a.b[0]}}?k={{ a }}")).toEqual([
      { start: 0, end: 5, name: "a" },
      { start: 8, end: 18, name: "a.b[0]" },
      { start: 21, end: 28, name: "a" },
    ]);
    expect(tokenize("{{a} and {{")).toEqual([]);
    expect(tokenize("{{}}")).toEqual([]);
  });
});

describe("classify", () => {
  it("defined, setLater and undefined, by root name", () => {
    expect(classify("a", dto)).toBe("defined");
    expect(classify("a.b[0]", dto)).toBe("defined");
    expect(classify("process.env.HOME", dto)).toBe("defined");
    expect(classify("token", dto)).toBe("setLater");
    expect(classify("token.x", dto)).toBe("setLater");
    expect(classify("missing", dto)).toBe("undefined");
  });

  it("no_underline_before_first_vars", () => {
    expect(classify("a", undefined)).toBeNull();
    expect(explain("a", undefined)).toBeNull();
  });
});

describe("explain", () => {
  it("gives value, source and shadows, or why there is none", () => {
    expect(explain("a", dto)).toMatchObject({ kind: "defined", value: "{\"b\":[1]}", source: "environment", shadows: ["collection"] });
    expect(explain("apiKey", dto)).toMatchObject({ kind: "defined", value: null, masked: true });
    expect(explain("token", dto)).toMatchObject({ kind: "setLater", setBy: "auth/login.bru" });
    expect(explain("missing", dto)).toMatchObject({ kind: "undefined" });
  });
});
