import { describe, expect, it } from "vitest";
import { pathParamNames, syncPathParams, syncQueryFromUrl, urlWithQuery } from "./url";

const kv = (name: string, value: string, enabled = true) => ({ name, value, enabled, local: false });

describe("url helpers", () => {
  it("rebuilds the query string from enabled params", () => {
    expect(urlWithQuery("http://h/x?old=1", [kv("a", "1"), kv("b", "2", false), kv("c", "")])).toBe(
      "http://h/x?a=1&c=",
    );
    expect(urlWithQuery("{{host}}/x", [])).toBe("{{host}}/x");
    expect(urlWithQuery("http://h/x?a=1#frag", [kv("z", "9")])).toBe("http://h/x?z=9#frag");
  });

  it("reads params from the URL and keeps disabled rows", () => {
    expect(syncQueryFromUrl("http://h/x?a=1&b={{v}}&flag", [kv("old", "0", false)])).toEqual([
      kv("old", "0", false),
      kv("a", "1"),
      kv("b", "{{v}}"),
      kv("flag", ""),
    ]);
    expect(syncQueryFromUrl("http://h/x", [kv("a", "1")])).toEqual([]);
  });

  it("keeps disabled rows in place when the URL has no query", () => {
    expect(syncQueryFromUrl("https://x/api", [kv("debug", "1", false)])).toEqual([kv("debug", "1", false)]);
  });

  it("keeps disabled rows in their positions; enabled rows follow the URL", () => {
    const rows = [kv("a", "1"), kv("b", "2", false), kv("c", "3")];
    expect(syncQueryFromUrl("http://h/x?a=1&c=3", rows)).toEqual(rows);
    expect(syncQueryFromUrl("http://h/x?a=9&c=3&d=4", rows)).toEqual([
      kv("a", "9"),
      kv("b", "2", false),
      kv("c", "3"),
      kv("d", "4"),
    ]);
    // Removing c from the URL drops the enabled row only.
    expect(syncQueryFromUrl("http://h/x?a=1", rows)).toEqual([kv("a", "1"), kv("b", "2", false)]);
  });

  it("finds :path params and keeps existing values", () => {
    expect(pathParamNames("{{host}}/users/:id/posts/:postId?x=:not")).toEqual(["id", "postId"]);
    expect(syncPathParams("http://h/users/:id/:other", [kv("id", "7"), kv("gone", "1")])).toEqual([
      kv("id", "7"),
      kv("other", ""),
    ]);
  });
});
