import { describe, expect, it } from "vitest";
import { jsonProblem, prettifyJson } from "./prettyJson";

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

describe("jsonProblem", () => {
  it("accepts JSON with quoted and unquoted {{variables}}, and an empty body", () => {
    for (const ok of ['{"id": {{id}}, "url": "{{host}}/x", "list": [{{a}}, 1]}', "", "  \n", "{{body}}"]) {
      expect(jsonProblem(ok), ok).toBeNull();
    }
  });

  it("points at a trailing comma", () => {
    const text = '{\n  "a": 1,\n}';
    expect(jsonProblem(text)).toEqual({ start: text.indexOf(","), end: text.indexOf(",") + 1, message: "Trailing comma" });
    expect(jsonProblem("[1,]")).toEqual({ start: 2, end: 3, message: "Trailing comma" });
  });

  it("points at the last character when a brace is missing", () => {
    const text = '{"a": {{id}}\n';
    expect(jsonProblem(text)).toEqual({ start: text.trimEnd().length - 1, end: text.trimEnd().length, message: "Expected ',' or '}'" });
  });

  it("points at the token that is wrong", () => {
    expect(jsonProblem('{"a" 1}')).toEqual({ start: 5, end: 6, message: "Expected ':'" });
    expect(jsonProblem("[1 2]")).toEqual({ start: 3, end: 4, message: "Expected ',' or ']'" });
    expect(jsonProblem("{a:1}")).toEqual({ start: 1, end: 2, message: "Unexpected character" });
    expect(jsonProblem('{"a":1} x')).toEqual({ start: 8, end: 9, message: "Unexpected character" });
    expect(jsonProblem('{"a":1} 2')).toEqual({ start: 8, end: 9, message: "Expected end of JSON" });
    expect(jsonProblem('{"a":}')).toEqual({ start: 5, end: 6, message: "Expected a value" });
  });
});
