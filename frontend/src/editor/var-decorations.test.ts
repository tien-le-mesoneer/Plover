import { describe, expect, it } from "vitest";
import type { VarsDto } from "../api/types";
import { varDecorations, varHoverAt, type TextModel } from "./varDecorations";

const dto: VarsDto = {
  vars: [
    { name: "host", source: "environment", value: "https://example.test", masked: false, truncated: false, shadows: [] },
    { name: "apiKey", source: "environment", value: null, masked: true, truncated: false, shadows: [] },
  ],
  setLater: [{ name: "token", by: "auth/login.bru" }],
};

/** Offsets to 1-based line/column like Monaco. */
function model(text: string): TextModel {
  return {
    getValue: () => text,
    getPositionAt(offset) {
      const before = text.slice(0, offset).split("\n");
      return { lineNumber: before.length, column: before[before.length - 1]!.length + 1 };
    },
  };
}

describe("body editor variables", () => {
  it("decorations_for_three_classes", () => {
    const m = model('{\n  "url": "{{host}}",\n  "t": "{{token}}", "x": "{{missing}}"\n}');
    const d = varDecorations(m, dto);
    expect(d.map((x) => x.options.inlineClassName)).toEqual(["var var-defined", "var var-setlater", "var var-undefined"]);
    expect(d[0]!.range).toEqual({ startLineNumber: 2, startColumn: 11, endLineNumber: 2, endColumn: 19 });
  });

  it("none_before_first_vars", () => {
    expect(varDecorations(model("{{host}}"), undefined)).toEqual([]);
    expect(varHoverAt(model("{{host}}"), 3, undefined, null)).toBeNull();
  });

  it("hover_markdown_masks_secret", () => {
    const h = varHoverAt(model('{"k": "{{apiKey}}"}'), 10, dto, "Dev")!;
    expect(h.markdown).toContain("•••• secret");
    expect(h.markdown).toContain("Environment: Dev");
  });

  it("hover_markdown_set_later", () => {
    const h = varHoverAt(model("{{token}}"), 2, dto, null)!;
    expect(h.markdown).toContain("Not set yet. Set at runtime by auth/login.bru.");
    expect(varHoverAt(model("plain {{token}}"), 1, dto, null)).toBeNull();
  });
});
