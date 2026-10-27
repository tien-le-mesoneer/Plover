import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiClient } from "../../api/client";
import { setApi } from "../../api/instance";
import type { HttpRequestDto } from "../../api/types";
import { useTabs } from "../../store/tabs";
import { useVars } from "../../store/vars";
import { useWorkspace } from "../../store/workspace";
import { KeyValueTable } from "./KeyValueTable";

let writeText: ReturnType<typeof vi.fn>;
let resolveText: ReturnType<typeof vi.fn>;
const draft = { name: "r" } as HttpRequestDto;

beforeEach(() => {
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  resolveText = vi.fn(async (_id: string, body: { text: string }) => ({ text: body.text.replace("{{id}}", "42"), containsSecret: false }));
  setApi({ resolveText } as unknown as ApiClient);
  useTabs.setState({ tabs: [{ key: "c1:a.bru", collectionId: "c1", item: "a.bru", saved: draft, draft, hash: "h", sending: false }] });
  useWorkspace.setState({ activeEnv: { c1: null } });
  useVars.setState({ byTab: {} });
});

afterEach(() => cleanup());

describe("param value copy", () => {
  it("param_row_copy_resolves_value", async () => {
    render(
      <KeyValueTable
        varTabKey="c1:a.bru"
        copyValues={{ tabKey: "c1:a.bru" }}
        rows={[{ name: "id", value: "{{id}}", enabled: true, local: false }]}
        onChange={() => {}}
      />,
    );
    const button = screen.getByRole("button", { name: "Copy value of id" });
    expect(button.closest("td")!.className).toContain("row-copy");
    await act(async () => {
      fireEvent.click(button);
    });
    expect(resolveText).toHaveBeenCalledWith("c1", expect.objectContaining({ text: "{{id}}", reveal: false }));
    expect(writeText).toHaveBeenCalledWith("42");
    expect(screen.getByText("Copied value of id")).toBeTruthy();
  });

  it("no copy column without copyValues, none on the new row", () => {
    const { unmount } = render(
      <KeyValueTable rows={[{ name: "id", value: "1", enabled: true, local: false }]} onChange={() => {}} />,
    );
    expect(screen.queryByRole("button", { name: /Copy value/ })).toBeNull();
    unmount();
    render(
      <KeyValueTable copyValues={{ tabKey: "c1:a.bru" }} rows={[{ name: "id", value: "1", enabled: true, local: false }]} onChange={() => {}} />,
    );
    expect(screen.getAllByRole("button", { name: /Copy value/ })).toHaveLength(1);
  });
});
