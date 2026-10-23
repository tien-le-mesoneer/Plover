import { useEffect, useState } from "react";
import { api } from "./api/instance";
import { handleServerEvent } from "./store/events";
import { Banner } from "./components/Banner";
import { EmptyState } from "./components/EmptyState";
import { CloseIcon } from "./components/icons";
import { Modal } from "./components/Modal";
import { closeTabWithConfirm } from "./components/RequestTabs";
import { EnvSelector } from "./components/EnvSelector";
import { RequestEditor } from "./components/request/RequestEditor";
import { RequestTabs } from "./components/RequestTabs";
import { RunPanel } from "./components/RunPanel";
import { Sidebar } from "./components/Sidebar";
import { SidebarHeader } from "./components/SidebarHeader";
import { useRuns } from "./store/runs";
import { useTabs } from "./store/tabs";
import { restoreUiState, startUiPersistence } from "./store/uiState";
import { useWorkspace } from "./store/workspace";

export default function App({ hasToken }: { hasToken: boolean }) {
  const { tabs, active, save, send } = useTabs();
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
    let stopPersisting: (() => void) | undefined;
    let cancelled = false;
    void init()
      .then(restoreUiState)
      .then(() => {
        // Save layout changes only after the remembered layout is back, so a restart
        // never overwrites it with an empty one.
        if (!cancelled) stopPersisting = startUiPersistence();
        return api().clientReady().catch(() => undefined);
      });
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
    return () => {
      cancelled = true;
      stopPersisting?.();
      events.close();
    };
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

  // App shortcuts: Cmd/Ctrl+S save, Cmd/Ctrl+Enter send, Cmd/Ctrl+W close the tab.
  // Monaco handles Cmd/Ctrl+Enter itself and dispatches "plover:send" (see CodeEditor).
  // Browsers reserve Cmd/Ctrl+W for closing the window and never deliver it to the
  // page; it works in the desktop app and in browsers that pass it through.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "s") {
        e.preventDefault();
        if (active) void save(active);
      } else if (key === "enter") {
        if (!active) return;
        e.preventDefault();
        void send(active);
      } else if (key === "w" && active && !e.shiftKey) {
        e.preventDefault();
        closeTabWithConfirm(active);
      }
    };
    const onSend = () => {
      if (active) void send(active);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("plover:send", onSend);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("plover:send", onSend);
    };
  }, [active, save, send]);

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
        <SidebarHeader />
        <span className="spacer" />
        {capabilities?.desktop && (
          <button type="button" onClick={() => void openInBrowser()}>
            Open in browser
          </button>
        )}
        <EnvSelector collectionId={collectionId} />
      </header>
      {confirmReload && (
        <Modal title="Reload window" onDismiss={() => setConfirmReload(false)}>
          <h3>Discard unsaved changes?</h3>
          <p>Some requests have unsaved edits. Reloading the window discards them.</p>
          <div className="actions">
            <button onClick={() => setConfirmReload(false)}>Cancel</button>
            <button className="primary" onClick={() => window.location.reload()}>
              Discard and reload
            </button>
          </div>
        </Modal>
      )}
      {error && (
        <Banner
          kind="error"
          role="alert"
          actions={
            <button className="icon" title="Dismiss" aria-label="Dismiss" onClick={clearError}>
              <CloseIcon />
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
          {tab ? <RequestEditor key={tab.key} tab={tab} /> : <EmptyState />}
          {showRun && <RunPanel onClose={() => setShowRun(false)} />}
        </main>
      </div>
    </div>
  );
}
