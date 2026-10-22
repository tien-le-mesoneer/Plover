import { describe, expect, it, vi } from "vitest";
import { takeTokenFromLocation } from "./token";

function fakeLocation(href: string): Location {
  const url = new URL(href);
  return { hash: url.hash, pathname: url.pathname, search: url.search } as Location;
}

describe("takeTokenFromLocation", () => {
  it("reads #token=, stores it and clears the hash", () => {
    const storage = window.sessionStorage;
    storage.clear();
    const history = { replaceState: vi.fn() };
    const token = takeTokenFromLocation(
      fakeLocation("http://127.0.0.1:4000/c/x?q=1#token=abc"),
      storage,
      history,
    );
    expect(token).toBe("abc");
    expect(storage.getItem("bru-rs.token")).toBe("abc");
    expect(history.replaceState).toHaveBeenCalledWith(null, "", "/c/x?q=1");
  });

  it("falls back to the stored token after a reload", () => {
    const storage = window.sessionStorage;
    storage.clear();
    storage.setItem("bru-rs.token", "kept");
    const history = { replaceState: vi.fn() };
    expect(takeTokenFromLocation(fakeLocation("http://127.0.0.1:4000/"), storage, history)).toBe(
      "kept",
    );
    expect(history.replaceState).not.toHaveBeenCalled();
  });

  it("returns null without a token", () => {
    const storage = window.sessionStorage;
    storage.clear();
    expect(
      takeTokenFromLocation(fakeLocation("http://127.0.0.1:4000/#other=1"), storage, {
        replaceState: vi.fn(),
      }),
    ).toBeNull();
  });
});
