import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../api/client";
import { setApi } from "../api/instance";
import { StartupBoundary } from "./StartupBoundary";

afterEach(cleanup);

function Broken(): never {
  throw new Error("cannot read 'x'");
}

describe("StartupBoundary", () => {
  it("shows a render crash and reports it as the startup error", () => {
    const clientReady = vi.fn(async () => undefined);
    setApi({ clientReady } as unknown as ApiClient);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <StartupBoundary>
        <Broken />
      </StartupBoundary>,
    );
    expect(screen.getByRole("alert").textContent).toContain("cannot read 'x'");
    expect(clientReady).toHaveBeenCalledWith("render failed: cannot read 'x'");
  });
});
