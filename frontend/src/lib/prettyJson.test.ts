import { describe, expect, it } from "vitest";
import { prettifyJson } from "./prettyJson";

describe("prettifyJson", () => {
  it("indents objects and arrays with 2 spaces", () => {
    expect(prettifyJson('{"a":1,"b":[true,null,{"c":"x"}],"d":{},"e":[]}')).toBe(
      ['{', '  "a": 1,', '  "b": [', "    true,", "    null,", "    {", '      "c": "x"', "    }", "  ],", '  "d": {},', '  "e": []', "}"].join("\n"),
    );
  });

  it("keeps unquoted and quoted {{variables}} byte for byte", () => {
    expect(prettifyJson('{"id":{{id}},"url":"{{host}}/x","list":[{{ a.b }}, {{c-d}}]}')).toBe(
      ['{', '  "id": {{id}},', '  "url": "{{host}}/x",', '  "list": [', "    {{ a.b }},", "    {{c-d}}", "  ]", "}"].join("\n"),
    );
  });

  it("keeps strings and numbers as written", () => {
    // JSON.parse would turn 1.0 into 1, lose the big id's precision and unescape é.
    expect(prettifyJson('{"n":1.0,"big":12345678901234567890,"s":"\\u00e9\\/\\"q"}')).toBe(
      ['{', '  "n": 1.0,', '  "big": 12345678901234567890,', '  "s": "\\u00e9\\/\\"q"', "}"].join("\n"),
    );
  });

  it("keeps a final newline", () => {
    expect(prettifyJson("[1, 2]\n")).toBe("[\n  1,\n  2\n]\n");
  });

  it("formats a bare value", () => {
    expect(prettifyJson("  {{body}}  ")).toBe("{{body}}");
    expect(prettifyJson(' "x" ')).toBe('"x"');
  });

  it("returns null for text that is not JSON", () => {
    for (const bad of ["", "{", '{"a":}', '{"a":1,}', "[1 2]", '{"a" 1}', "{a:1}", '{"a":1} x', "'x'", "01", '"a\nb"']) {
      expect(prettifyJson(bad), bad).toBeNull();
    }
  });
});
