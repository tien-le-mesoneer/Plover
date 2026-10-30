import { describe, expect, it } from "vitest";
import { jsonMarkers } from "./jsonMarkers";
import type { TextModel } from "./varDecorations";

/** A text model over a string, with 1-based lines and columns like Monaco's. */
const model = (text: string): TextModel => ({
  getValue: () => text,
  getPositionAt: (offset) => {
    const before = text.slice(0, offset).split("\n");
    return { lineNumber: before.length, column: before[before.length - 1]!.length + 1 };
  },
});

describe("jsonMarkers", () => {
  it("leaves {{variables}} unmarked", () => {
    expect(jsonMarkers(model('{\n  "id": {{id}},\n  "url": "{{host}}/x"\n}'))).toEqual([]);
  });

  it("marks a trailing comma on its line and column", () => {
    expect(jsonMarkers(model('{\n  "id": {{id}},\n}'))).toEqual([
      { startLineNumber: 2, startColumn: 15, endLineNumber: 2, endColumn: 16, message: "Trailing comma" },
    ]);
  });

  it("marks the end when a brace is missing", () => {
    expect(jsonMarkers(model('{\n  "a": [1, 2]\n'))).toEqual([
      { startLineNumber: 2, startColumn: 13, endLineNumber: 2, endColumn: 14, message: "Expected ',' or '}'" },
    ]);
  });
});
