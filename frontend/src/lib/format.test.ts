import { describe, expect, it } from "vitest";
import type { ResponseDto } from "../api/types";
import { formatDuration, formatSize, prettyBody } from "./format";

const res = (patch: Partial<ResponseDto>): ResponseDto => ({
  status: 200,
  headers: [],
  elapsedMs: 1,
  size: 0,
  sizeComplete: true,
  bodyText: "",
  bodyBase64: null,
  truncated: false,
  ...patch,
});

describe("format", () => {
  it("formats sizes in base 1024 with one decimal", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1023)).toBe("1023 B");
    expect(formatSize(1229)).toBe("1.2 KB");
    expect(formatSize(3.4 * 1024 * 1024)).toBe("3.4 MB");
  });

  it("formats durations", () => {
    expect(formatDuration(85)).toBe("85 ms");
    expect(formatDuration(1240)).toBe("1.24 s");
  });

  it("pretty-prints JSON by content type", () => {
    const r = res({ headers: [["Content-Type", "application/json; charset=utf-8"]], bodyText: '{"a":[1,2]}' });
    expect(prettyBody(r)).toEqual({ language: "json", text: '{\n  "a": [\n    1,\n    2\n  ]\n}' });
  });

  it("keeps invalid JSON as text in the json language", () => {
    const r = res({ headers: [["content-type", "application/json"]], bodyText: "{oops" });
    expect(prettyBody(r)).toEqual({ language: "json", text: "{oops" });
  });

  it("detects xml, html and plain text", () => {
    expect(prettyBody(res({ headers: [["content-type", "application/xml"]], bodyText: "<a/>" })).language).toBe("xml");
    expect(prettyBody(res({ headers: [["content-type", "text/html"]], bodyText: "<p>" })).language).toBe("html");
    expect(prettyBody(res({ bodyText: "hi" }))).toEqual({ language: "text", text: "hi" });
  });

  it("does not render base64 bodies", () => {
    const r = res({ bodyText: null, bodyBase64: "AAEC", size: 3 });
    expect(prettyBody(r)).toEqual({ language: "binary", text: "Binary body (3 B) — not shown." });
  });
});
