import { describe, expect, it } from "vitest";
import type { VarsDto } from "../api/types";
import { acceptSuggestion, openToken, registerVarCompletion, suggestions } from "./varSuggest";

const dto: VarsDto = {
  vars: [
    { name: "host", source: "environment", value: "api.test", masked: false, truncated: false, shadows: [] },
    { name: "apiKey", source: "environment", value: null, masked: true, truncated: false, shadows: [] },
    { name: "hostPort", source: "collection", value: "8080", masked: false, truncated: false, shadows: [] },
    { name: "theHost", source: "runtime", value: "x", masked: false, truncated: false, shadows: [] },
    { name: "process.env.DB_PASS", source: "processEnv", value: null, masked: true, truncated: false, shadows: [] },
  ],
  setLater: [{ name: "token", by: "auth/login.bru" }],
};

describe("variable suggestions", () => {
  it("prefix matches come before fuzzy ones, with scope labels", () => {
    const s = suggestions(dto, "ho", "Dev");
    expect(s.map((x) => x.name)).toEqual(["host", "hostPort", "theHost"]);
    expect(s[0]).toEqual({ name: "host", label: "Environment: Dev" });
    expect(s[1]!.label).toBe("Collection");
  });

  it("set_later_names_labelled and process env left out", () => {
    const all = suggestions(dto, "", null);
    expect(all.find((x) => x.name === "token")).toEqual({ name: "token", label: "set later" });
    expect(all.some((x) => x.name.startsWith("process.env"))).toBe(false);
    // No value field, by type and at run time.
    expect(Object.keys(all[0]!).sort()).toEqual(["label", "name"]);
  });

  it("at most 50, none before the first answer", () => {
    const many: VarsDto = {
      vars: Array.from({ length: 80 }, (_, i) => ({ name: `v${i}`, source: "collection", value: "1", masked: false, truncated: false, shadows: [] })),
      setLater: [],
    };
    expect(suggestions(many, "", null)).toHaveLength(50);
    expect(suggestions(undefined, "", null)).toEqual([]);
  });

  it("openToken finds the open {{ under the caret", () => {
    expect(openToken("https://{{ho", 12)).toEqual({ start: 8, query: "ho" });
    expect(openToken("{{ho}}", 4)).toEqual({ start: 0, query: "ho" });
    expect(openToken("{{host}} x", 10)).toBeNull();
    expect(openToken("{ x", 3)).toBeNull();
    expect(openToken("{{a b", 5)).toBeNull();
  });

  it("acceptSuggestion completes the name and closes it once", () => {
    expect(acceptSuggestion("https://{{ho", 12, "host")).toEqual({ text: "https://{{host}}", caret: 16 });
    expect(acceptSuggestion("{{ho}}", 4, "host")).toEqual({ text: "{{host}}", caret: 8 });
    expect(acceptSuggestion("{{h/x", 3, "host")).toEqual({ text: "{{host}}/x", caret: 8 });
  });

  it("monaco_provider_returns_same_names", () => {
    let provider: { triggerCharacters?: string[]; provideCompletionItems: (m: unknown, p: unknown) => { suggestions: { label: string; kind: number; detail: string; insertText: string }[] } } | null = null;
    const languages: string[] = [];
    const fake = {
      languages: {
        CompletionItemKind: { Variable: 4 },
        registerCompletionItemProvider: (lang: string, p: typeof provider) => {
          languages.push(lang);
          provider = p;
          return { dispose() {} };
        },
      },
    };
    registerVarCompletion(fake as never, () => ({ vars: dto, env: "Dev" }));
    expect(languages).toEqual(["json", "plaintext", "xml"]);
    expect(provider!.triggerCharacters).toContain("{");
    const text = '{"h": "{{ho';
    const model = {
      getValue: () => text,
      getOffsetAt: () => text.length,
      getPositionAt: (o: number) => ({ lineNumber: 1, column: o + 1 }),
    };
    const items = provider!.provideCompletionItems(model, { lineNumber: 1, column: text.length + 1 }).suggestions;
    expect(items.map((i) => i.label)).toEqual(suggestions(dto, "ho", "Dev").map((s) => s.name));
    expect(items[0]).toMatchObject({ kind: 4, detail: "Environment: Dev", insertText: "host}}" });
  });
});
