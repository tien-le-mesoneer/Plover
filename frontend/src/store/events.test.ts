import { describe, expect, it, vi } from "vitest";
import { handleServerEvent } from "./events";

function deps(dirty: boolean) {
  return {
    anyDirty: () => dirty,
    reload: vi.fn(),
    askReload: vi.fn(),
    openedElsewhere: vi.fn(async () => {}),
  };
}

describe("server events", () => {
  it("adds a collection opened natively without reloading", () => {
    const d = deps(true);
    handleServerEvent({ type: "collection_opened", id: "c2" }, d);
    expect(d.openedElsewhere).toHaveBeenCalledWith("c2");
    expect(d.reload).not.toHaveBeenCalled();
  });

  it("reloads at once when nothing is unsaved", () => {
    const d = deps(false);
    handleServerEvent({ type: "reload_requested" }, d);
    expect(d.reload).toHaveBeenCalled();
    expect(d.askReload).not.toHaveBeenCalled();
  });

  it("asks before reloading over unsaved edits", () => {
    const d = deps(true);
    handleServerEvent({ type: "reload_requested" }, d);
    expect(d.askReload).toHaveBeenCalled();
    expect(d.reload).not.toHaveBeenCalled();
  });
});
