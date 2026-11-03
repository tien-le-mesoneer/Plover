import { describe, expect, it, vi } from "vitest";
import { resolveSessionToken, takeLaunchCode, takeTokenFromLocation } from "./token";

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

describe("resolveSessionToken", () => {
  const deps = (href: string, over: { exchange?: () => Promise<string>; open?: () => Promise<string> } = {}) => {
    const storage = window.sessionStorage;
    return {
      loc: fakeLocation(href),
      storage,
      history: { replaceState: vi.fn() },
      exchange: vi.fn(over.exchange ?? (async () => "from-code")),
      open: vi.fn(over.open ?? (async () => "from-server")),
    };
  };

  it("opens the plain address with the server's token", async () => {
    window.sessionStorage.clear();
    window.sessionStorage.setItem("plover.token", "stale");
    const d = deps("http://127.0.0.1:4000/");
    expect(await resolveSessionToken(d)).toBe("from-server");
    expect(window.sessionStorage.getItem("plover.token")).toBe("from-server");
    expect(d.exchange).not.toHaveBeenCalled();
  });

  it("prefers a launch code, then #token=", async () => {
    window.sessionStorage.clear();
    const viaCode = deps("http://127.0.0.1:4000/#code=c0de");
    expect(await resolveSessionToken(viaCode)).toBe("from-code");
    expect(viaCode.exchange).toHaveBeenCalledWith("c0de");
    expect(viaCode.open).not.toHaveBeenCalled();
    const viaHash = deps("http://127.0.0.1:4000/#token=abc");
    expect(await resolveSessionToken(viaHash)).toBe("abc");
    expect(viaHash.open).not.toHaveBeenCalled();
  });

  it("falls back to the stored token, or none, when the server asks for one", async () => {
    window.sessionStorage.clear();
    const refuse = { open: async () => Promise.reject(new Error("404")) };
    expect(await resolveSessionToken(deps("http://127.0.0.1:4000/", refuse))).toBeNull();
    window.sessionStorage.setItem("plover.token", "kept");
    expect(await resolveSessionToken(deps("http://127.0.0.1:4000/", refuse))).toBe("kept");
  });
});
