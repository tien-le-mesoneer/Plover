import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const root = join(__dirname, "..");

/** Every component source file (tests excluded). */
function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return e.name.endsWith(".tsx") && !e.name.includes(".test.") ? [path] : [];
  });
}

/** `file:line` of each `<button>` element without a `type` attribute. */
function untypedButtons(file: string): string[] {
  const source = ts.createSourceFile(file, readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const found: string[] = [];
  const visit = (node: ts.Node) => {
    if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && node.tagName.getText() === "button") {
      const typed = node.attributes.properties.some((p) => ts.isJsxAttribute(p) && p.name.getText() === "type");
      if (!typed) found.push(`${relative(root, file)}:${source.getLineAndCharacterOfPosition(node.getStart()).line + 1}`);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return found;
}

describe("buttons", () => {
  it("every_button_states_its_type", () => {
    // A `<button>` without a type submits the form it sits in.
    expect(sources(root).flatMap(untypedButtons)).toEqual([]);
  });
});
