import { useEffect, useState } from "react";
import { Banner } from "./components/Banner";
import { EnvSelector } from "./components/EnvSelector";
import { RequestEditor } from "./components/request/RequestEditor";
import { RequestTabs } from "./components/RequestTabs";
import { RunPanel } from "./components/RunPanel";
import { Sidebar } from "./components/Sidebar";
import { useRuns } from "./store/runs";
import { useTabs } from "./store/tabs";
import { useWorkspace } from "./store/workspace";

export default function App({ hasToken }: { hasToken: boolean }) {
  const { tabs, active, save } = useTabs();
  const { collections, activeEnv, error, clearError, init } = useWorkspace();
  const startRun = useRuns((s) => s.start);
  const [showRun, setShowRun] = useState(false);
  const tab = tabs.find((t) => t.key === active) ?? null;
  const collectionId = tab?.collectionId ?? collections[0]?.id ?? null;

  useEffect(() => {
    if (hasToken) void init();
  }, [hasToken, init]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (active) void save(active);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, save]);

  if (!hasToken) {
    return (
      <main className="empty">
        <h1>bru-rs</h1>
        <p>Open the URL printed by <code>bru-rs serve</code> (it contains the session token).</p>
      </main>
    );
  }

  return (
    <div className="app">
      <header className="topbar">
        <span className="logo">bru-rs</span>
        <span className="spacer" />
        <EnvSelector collectionId={collectionId} />
      </header>
      {error && (
        <Banner
          kind="error"
          role="alert"
          actions={
            <button className="icon" title="Dismiss" aria-label="Dismiss" onClick={clearError}>
              ×
            </button>
          }
        >
          {error}
        </Banner>
      )}
      <div className="layout">
        <Sidebar
          onRun={(id, item, label) => {
            startRun(id, item, activeEnv[id] ?? null, label);
            setShowRun(true);
          }}
        />
        <main className="main">
          <RequestTabs />
          {tab ? (
            <RequestEditor key={tab.key} tab={tab} />
          ) : (
            <p className="muted center">
              {collections.length ? "Pick a request in the sidebar." : "Open a Bruno collection to start."}
            </p>
          )}
          {showRun && <RunPanel onClose={() => setShowRun(false)} />}
        </main>
      </div>
    </div>
  );
}
