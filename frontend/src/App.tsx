import { useEffect } from "react";
import { EnvSelector } from "./components/EnvSelector";
import { RequestEditor } from "./components/request/RequestEditor";
import { RequestTabs } from "./components/RequestTabs";
import { Sidebar } from "./components/Sidebar";
import { useTabs } from "./store/tabs";
import { useWorkspace } from "./store/workspace";

export default function App({ hasToken }: { hasToken: boolean }) {
  const { tabs, active, save } = useTabs();
  const { collections, error, clearError, init } = useWorkspace();
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
        <div className="banner error" role="alert">
          {error}
          <button className="icon" onClick={clearError}>
            ×
          </button>
        </div>
      )}
      <div className="layout">
        <Sidebar />
        <main className="main">
          <RequestTabs />
          {tab ? (
            <RequestEditor key={tab.key} tab={tab} />
          ) : (
            <p className="muted center">
              {collections.length ? "Pick a request in the sidebar." : "Open a Bruno collection to start."}
            </p>
          )}
        </main>
      </div>
    </div>
  );
}
