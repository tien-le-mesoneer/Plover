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

type Element = ts.JsxOpeningElement | ts.JsxSelfClosingElement;

/** A string attribute's value (`role="menu"`), or whether a non-string one is there. */
function attr(node: Element, name: string): string | boolean {
  const found = node.attributes.properties.find((p) => ts.isJsxAttribute(p) && p.name.getText() === name);
  if (!found || !ts.isJsxAttribute(found)) return false;
  const init = found.initializer;
  if (init && ts.isStringLiteral(init)) return init.text;
  if (init && ts.isJsxExpression(init) && init.expression && ts.isStringLiteralLike(init.expression)) return init.expression.text;
  return true;
}

/** `file:line` of each JSX element that `bad` flags. */
function offenders(bad: (node: Element) => boolean) {
  return (file: string): string[] => {
    const source = ts.createSourceFile(file, readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const found: string[] = [];
    const visit = (node: ts.Node) => {
      if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && bad(node)) {
        found.push(`${relative(root, file)}:${source.getLineAndCharacterOfPosition(node.getStart()).line + 1}`);
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
    return found;
  };
}

const hasClass = (node: Element, name: string) => {
  const cls = attr(node, "className");
  return typeof cls === "string" && cls.split(/\s+/).includes(name);
};

describe("buttons", () => {
  it("every_button_states_its_type", () => {
    // A `<button>` without a type submits the form it sits in.
    const untyped = offenders((n) => n.tagName.getText() === "button" && attr(n, "type") === false);
    expect(sources(root).flatMap(untyped)).toEqual([]);
  });
});

describe("menus and popovers", () => {
  it("every_menu_and_popover_is_a_surface", () => {
    // One border, radius, padding and shadow for everything that floats over the page.
    const floating = offenders((n) => (attr(n, "role") === "menu" || hasClass(n, "popover")) && !hasClass(n, "surface"));
    expect(sources(root).flatMap(floating)).toEqual([]);
  });

  it("every_menu_item_is_a_menu_item", () => {
    const items = offenders((n) => attr(n, "role") === "menuitem" && !hasClass(n, "menu-item"));
    expect(sources(root).flatMap(items)).toEqual([]);
  });
});
