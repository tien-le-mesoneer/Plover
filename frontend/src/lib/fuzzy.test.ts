import { describe, expect, it } from "vitest";
import { fold, rank, score, type QuickItem } from "./fuzzy";

const item = (name: string, path = "", collection = "api", id = name): QuickItem => ({
  collectionId: "c1",
  item: `${id}.bru`,
  method: "GET",
  name,
  path,
  collection,
});

describe("fuzzy", () => {
  it("folds case and diacritics", () => {
    expect(fold("Đăng NHẬP")).toBe("dang nhap");
  });

  it("ranks word-start matches in the name first", () => {
    const items = [item("assign document type"), item("Sign document")];
    expect(rank("sign doc", items).map((i) => i.name)).toEqual(["Sign document", "assign document type"]);
  });

  it("diacritics_and_case_insensitive", () => {
    expect(score("dang nhap", item("Đăng nhập"))).not.toBeNull();
    expect(score("LOGIN", item("login"))).not.toBeNull();
  });

  it("matches the folder path and collection when the name does not", () => {
    expect(score("usrlist", item("list", "users"))).not.toBeNull();
    expect(score("zzz", item("list", "users"))).toBeNull();
    expect(score("tsil", item("list"))).toBeNull();
  });

  it("rank_is_stable_on_ties", () => {
    const items = [item("login", "", "api", "a"), item("login", "", "api", "b"), item("login", "", "api", "c")];
    expect(rank("login", items).map((i) => i.item)).toEqual(["a.bru", "b.bru", "c.bru"]);
  });

  it("caps results", () => {
    const items = Array.from({ length: 80 }, (_, i) => item(`req ${i}`));
    expect(rank("req", items)).toHaveLength(50);
    expect(rank("req", items, 10)).toHaveLength(10);
  });

  it("rank_2000_under_16ms", () => {
    const words = ["user", "order", "sign", "document", "login", "token", "invoice", "status"];
    const items = Array.from({ length: 2000 }, (_, i) =>
      item(`${words[i % 8]} ${words[(i * 3) % 8]} ${i}`, `${words[(i * 5) % 8]}/${words[(i * 7) % 8]}`),
    );
    rank("signdo", items); // warm up the folded cache
    const times: number[] = [];
    for (let run = 0; run < 20; run++) {
      const start = performance.now();
      rank("signdo", items);
      times.push(performance.now() - start);
    }
    times.sort((a, b) => a - b);
    expect(times[10]!).toBeLessThan(16);
  });
});
