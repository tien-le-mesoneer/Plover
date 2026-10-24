import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { EnvironmentDto } from "../api/types";
import { useWorkspace } from "../store/workspace";
import { EnvSelector } from "./EnvSelector";

const env = (name: string) => ({ name, vars: [], secrets: [], hash: "h" }) as unknown as EnvironmentDto;

beforeEach(() => {
  useWorkspace.setState({
    collections: [
      { id: "a", name: "signing-api", root: "/a", available: true, error: null },
      { id: "b", name: "billing", root: "/b", available: true, error: null },
    ],
    environments: { a: [env("Dev"), env("Stg")], b: [env("Dev")] },
    activeEnv: { a: "Dev", b: "Dev" },
  });
});

afterEach(cleanup);

describe("environment label in the top bar", () => {
  it("label_shows_collection_and_env", () => {
    render(<EnvSelector collectionId="a" />);
    expect(screen.getByText("signing-api")).toBeTruthy();
    const select = screen.getByRole("combobox", { name: "Environment for signing-api" }) as HTMLSelectElement;
    expect(select.value).toBe("Dev");
  });

  it("label_follows_collection_with_same_env_name", () => {
    const { rerender } = render(<EnvSelector collectionId="a" />);
    expect(screen.getByRole("combobox", { name: "Environment for signing-api" })).toBeTruthy();
    rerender(<EnvSelector collectionId="b" />);
    expect(screen.getByText("billing")).toBeTruthy();
    expect(screen.queryByText("signing-api")).toBeNull();
    expect((screen.getByRole("combobox", { name: "Environment for billing" }) as HTMLSelectElement).value).toBe("Dev");
  });

  it("no_confirmation_for_stg", () => {
    render(<EnvSelector collectionId="a" />);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "Stg" } });
    expect(useWorkspace.getState().activeEnv.a).toBe("Stg");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("long_collection_name_truncates_first", () => {
    useWorkspace.setState({
      collections: [{ id: "a", name: "a-very-long-collection-name-that-will-not-fit", root: "/a", available: true, error: null }],
    });
    render(<EnvSelector collectionId="a" />);
    const name = screen.getByText("a-very-long-collection-name-that-will-not-fit");
    expect(name.className).toContain("env-collection");
    expect(name.getAttribute("title")).toBe("a-very-long-collection-name-that-will-not-fit");
  });
});
