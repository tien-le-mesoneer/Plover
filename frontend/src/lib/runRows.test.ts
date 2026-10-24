import { describe, expect, it } from "vitest";
import type { RunEventDto } from "../api/types";
import { buildRows, duplicateNames, filterCounts } from "./runRows";

const started = (item: string, name: string): RunEventDto => ({ type: "started", runId: "r", iteration: 0, item, name, method: "POST", url: "u" });
const passed = (index: number, item: string): RunEventDto => ({
  type: "passed", runId: "r", index, iteration: 0, item, status: 200, elapsedMs: 5, size: 2, tests: [], asserts: [], console: [],
});
const failed = (index: number, item: string): RunEventDto => ({
  type: "failed", runId: "r", index, iteration: 0, item, error: "boom", status: 500, elapsedMs: 5, tests: [], asserts: [], console: [], scriptError: null,
});

describe("run rows", () => {
  it("keys rows by result index for repeated items", () => {
    const rows = buildRows([started("a/login.bru", "login"), passed(0, "a/login.bru"), started("a/login.bru", "login"), failed(1, "a/login.bru")]);
    expect(rows.map((r) => [r.index, r.state, r.folder])).toEqual([
      [0, "pass", "a"],
      [1, "fail", "a"],
    ]);
  });

  it("duplicateNames finds names on two or more rows", () => {
    const rows = buildRows([
      started("p1/login.bru", "login"), passed(0, "p1/login.bru"),
      started("p2/login.bru", "login"), passed(1, "p2/login.bru"),
      started("p7/login.bru", "login"), passed(2, "p7/login.bru"),
      started("me.bru", "me"), passed(3, "me.bru"),
    ]);
    expect(duplicateNames(rows)).toEqual(new Set(["login"]));
  });

  it("folder_line_only_for_duplicates_live", () => {
    const events: RunEventDto[] = [
      started("p1/login.bru", "login"), passed(0, "p1/login.bru"),
      started("me.bru", "me"), passed(1, "me.bru"),
      started("p2/login.bru", "login"), passed(2, "p2/login.bru"),
    ];
    const seen: string[][] = [];
    for (let n = 1; n <= events.length; n++) seen.push([...duplicateNames(buildRows(events.slice(0, n)))]);
    expect(seen.slice(0, 4)).toEqual([[], [], [], []]);
    expect(seen[4]).toEqual(["login"]);
    expect(seen[5]).toEqual(["login"]);
  });

  it("running rows count only in All", () => {
    const rows = buildRows([started("a.bru", "a"), passed(0, "a.bru"), started("b.bru", "b"), failed(1, "b.bru"), started("c.bru", "c")]);
    expect(filterCounts(rows)).toEqual({ all: 3, fail: 1, pass: 1, skip: 0 });
  });
});
