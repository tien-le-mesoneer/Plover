import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ConsoleOutput } from "./ConsoleOutput";
import { TestResults, testSummary } from "./TestResults";

afterEach(cleanup);

const tests = [
  { name: "status is 200", passed: true, error: null },
  { name: "has token", passed: false, error: "expected undefined to equal 'abc'" },
];
const asserts = [{ lhs: "res.status", rhs: "eq 200", operator: "eq", passed: true, error: null }];

describe("TestResults", () => {
  it("counts passed tests and asserts together", () => {
    expect(testSummary(tests, asserts)).toEqual({ passed: 2, total: 3 });
    expect(testSummary([], [])).toEqual({ passed: 0, total: 0 });
  });

  it("lists asserts and tests with failure messages", () => {
    render(<TestResults tests={tests} asserts={asserts} />);
    expect(screen.getByText("2/3 passed")).toBeTruthy();
    expect(screen.getByText("res.status: eq 200")).toBeTruthy();
    expect(screen.getByText("status is 200")).toBeTruthy();
    expect(screen.getByText("expected undefined to equal 'abc'")).toBeTruthy();
    const failed = screen.getByText("has token").closest("li")!;
    expect(failed.className).toContain("fail");
  });

  it("says when there is nothing to show", () => {
    render(<TestResults tests={[]} asserts={[]} />);
    expect(screen.getByText(/No tests or asserts/)).toBeTruthy();
  });
});

describe("ConsoleOutput", () => {
  it("shows each line with its level", () => {
    render(
      <ConsoleOutput
        lines={[
          { level: "log", text: "hello" },
          { level: "error", text: "boom" },
        ]}
      />,
    );
    expect(screen.getByText("hello").closest("li")!.className).toContain("log");
    expect(screen.getByText("boom").closest("li")!.className).toContain("error");
  });
});

describe("ConsoleOutput scripted requests", () => {
  it("lists requests scripts sent, with failures marked", () => {
    render(
      <ConsoleOutput
        lines={[]}
        requests={[
          { source: "sendRequest", method: "GET", url: "http://h/a", status: 200, error: null, durationMs: 3 },
          { source: "axios", method: "POST", url: "http://h/b", status: null, error: "refused", durationMs: 1 },
        ]}
      />,
    );
    expect(screen.getByText(/sendRequest GET http:\/\/h\/a → 200/)).toBeTruthy();
    expect(screen.getByText(/axios POST http:\/\/h\/b → error: refused/).closest("li")!.className).toContain("error");
  });
});
