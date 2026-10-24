import { describe, expect, it } from "vitest";
import { statusGroup, statusText } from "./status";

describe("status groups", () => {
  it("puts each boundary in its group", () => {
    expect(statusGroup(199)).toBe("info");
    expect(statusGroup(200)).toBe("ok");
    expect(statusGroup(299)).toBe("ok");
    expect(statusGroup(300)).toBe("info");
    expect(statusGroup(399)).toBe("info");
    expect(statusGroup(400)).toBe("warn");
    expect(statusGroup(499)).toBe("warn");
    expect(statusGroup(500)).toBe("err");
    expect(statusGroup(null)).toBe("err");
  });

  it("writes the status with its reason, or the error code", () => {
    expect(statusText(404)).toBe("404 Not Found");
    expect(statusText(200)).toBe("200 OK");
    expect(statusText(null, "ECONNREFUSED")).toBe("ECONNREFUSED");
    expect(statusText(null)).toBe("No response");
  });
});
