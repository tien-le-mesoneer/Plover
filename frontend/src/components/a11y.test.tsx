import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { HttpRequestDto } from "../api/types";
import { useTabs } from "../store/tabs";
import { useWorkspace } from "../store/workspace";
import { KeyValueTable } from "./request/KeyValueTable";
import { RequestTabs } from "./RequestTabs";
import { Sidebar } from "./Sidebar";

const tree = {
  kind: "folder" as const,
  id: "",
  name: "c",
  method: null,
  seq: null,
  error: null,
  children: [],
};

const request = { name: "list users", method: "GET" } as HttpRequestDto;

beforeEach(() => {
  useWorkspace.setState({
    collections: [{ id: "c1", name: "ui-check", root: "/x" }],
    trees: { c1: tree },
    environments: {},
    activeEnv: {},
  });
  useTabs.setState({
    tabs: [{ key: "c1:a.bru", collectionId: "c1", item: "a.bru", saved: request, draft: request, hash: "h", sending: false }],
    active: "c1:a.bru",
  });
});

afterEach(cleanup);

describe("icon-only buttons have accessible names", () => {
  it("sidebar collection actions", () => {
    render(<Sidebar onRun={() => {}} />);
    expect(screen.getByRole("button", { name: "Run collection" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reload from disk" })).toBeTruthy();
  });

  it("tab close button", () => {
    render(<RequestTabs />);
    const tab = screen.getByRole("tab", { name: /list users/ });
    expect(within(tab).getByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("key/value remove button", () => {
    render(<KeyValueTable rows={[{ name: "a", value: "1", enabled: true, local: false }]} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Remove" })).toBeTruthy();
  });
});

describe("banners", () => {
  it("the ~ warning is one block of text, not a row of flex items", () => {
    render(<KeyValueTable rows={[{ name: "~x", value: "", enabled: true, local: false }]} onChange={() => {}} />);
    const note = screen.getByRole("note");
    expect(note.children).toHaveLength(1);
    expect(note.children[0]!.textContent).toMatch(/^A name starting with ~ means .disabled. in \.bru; .* without the ~\.$/);
  });
});
