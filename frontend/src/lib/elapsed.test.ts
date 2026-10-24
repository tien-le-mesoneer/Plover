import { describe, expect, it } from "vitest";
import { formatElapsed } from "./elapsed";

describe("formatElapsed", () => {
  it("shows tenths under 10 s, whole seconds under a minute, then minutes", () => {
    expect(formatElapsed(0)).toBe("0.0s");
    expect(formatElapsed(3240)).toBe("3.2s");
    expect(formatElapsed(9999)).toBe("9.9s");
    expect(formatElapsed(10000)).toBe("10s");
    expect(formatElapsed(59999)).toBe("59s");
    expect(formatElapsed(65000)).toBe("1m 05s");
  });
});
