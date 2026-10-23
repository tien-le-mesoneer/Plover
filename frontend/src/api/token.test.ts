import { describe, expect, it, vi } from "vitest";
import { takeLaunchCode, takeTokenFromLocation } from "./token";

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
    expect(storage.getItem("plover.token")).toBe("abc");
    expect(history.replaceState).toHaveBeenCalledWith(null, "", "/c/x?q=1");
  });

  it("falls back to the stored token after a reload", () => {
    const storage = window.sessionStorage;
    storage.clear();
    storage.setItem("plover.token", "kept");
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

describe("takeLaunchCode", () => {
  it("reads #code= and clears the hash so it never stays in history", () => {
    const history = { replaceState: vi.fn() };
    expect(takeLaunchCode(fakeLocation("http://127.0.0.1:4000/x#code=c0de"), history)).toBe("c0de");
    expect(history.replaceState).toHaveBeenCalledWith(null, "", "/x");
  });

  it("returns null without a code", () => {
    const history = { replaceState: vi.fn() };
    expect(takeLaunchCode(fakeLocation("http://127.0.0.1:4000/#token=t"), history)).toBeNull();
    expect(history.replaceState).not.toHaveBeenCalled();
  });
});
