import { useEffect, useState } from "react";
import { api } from "./api/instance";
import { handleServerEvent } from "./store/events";
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
  const { collections, activeEnv, error, clearError, init, capabilities, openInBrowser, activeCollection } =
    useWorkspace();
  const anyDirty = useTabs((s) => s.tabs.some((t) => s.isDirty(t.key)));
  const [confirmReload, setConfirmReload] = useState(false);
  const startRun = useRuns((s) => s.start);
  const [showRun, setShowRun] = useState(false);
  const tab = tabs.find((t) => t.key === active) ?? null;
  const collectionId = tab?.collectionId ?? activeCollection ?? collections[0]?.id ?? null;

  useEffect(() => {
    if (!hasToken) return;
    void init().then(() => api().clientReady().catch(() => undefined));
    const events = api().subscribeEvents((event) =>
      handleServerEvent(event, {
        anyDirty: () => {
          const { tabs, isDirty } = useTabs.getState();
          return tabs.some((t) => isDirty(t.key));
        },
        reload: () => window.location.reload(),
        askReload: () => setConfirmReload(true),
        openedElsewhere: (id) => useWorkspace.getState().openedElsewhere(id),
      }),
    );
    return () => events.close();
  }, [hasToken, init]);

  // Closing or reloading the page (browser tab, window) asks first when edits are unsaved.
  useEffect(() => {
    if (!anyDirty) return;
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [anyDirty]);

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
        <h1>Plover</h1>
        <p>Open the URL printed by <code>plv serve</code> (it contains the session token).</p>
      </main>
    );
  }

  return (
    <div className="app">
      <header className="topbar">
        <span className="logo">Plover</span>
        <span className="spacer" />
        {capabilities?.desktop && (
          <button type="button" onClick={() => void openInBrowser()}>
            Open in browser
          </button>
        )}
        <EnvSelector collectionId={collectionId} />
      </header>
      {confirmReload && (
        <div className="modal-backdrop">
          <div className="modal" role="dialog" aria-label="Reload window">
            <h3>Discard unsaved changes?</h3>
            <p>Some requests have unsaved edits. Reloading the window discards them.</p>
            <div className="actions">
              <button onClick={() => setConfirmReload(false)}>Cancel</button>
              <button className="primary" onClick={() => window.location.reload()}>
                Discard and reload
              </button>
            </div>
          </div>
        </div>
      )}
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
