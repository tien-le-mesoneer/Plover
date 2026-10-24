// Test helper: a Run tab read from the tabs store, as App renders it.
import { useTabs } from "../../store/tabs";
import { RunTab } from "./RunTab";

export const HARNESS_KEY = "c1:run:harness";

function Shown() {
  const tab = useTabs((s) => s.runTabs.find((t) => t.key === HARNESS_KEY));
  return tab ? <RunTab tab={tab} /> : null;
}

/** Puts a Run tab for `runId` (collection `c1`) in the store and returns the element to render. */
export function runTabFor(runId: string | null = "r1", label = "all") {
  useTabs.setState({
    runTabs: [{ kind: "run", key: HARNESS_KEY, collectionId: "c1", runId, label }],
    active: HARNESS_KEY,
  });
  return <Shown />;
}
