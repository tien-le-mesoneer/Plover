import { useEffect, useState, type CSSProperties } from "react";
import { api } from "./api/instance";
import { handleServerEvent } from "./store/events";
import { Banner } from "./components/Banner";
import { EmptyState } from "./components/EmptyState";
import { CloseIcon } from "./components/icons";
import { Modal } from "./components/Modal";
import { closeTabWithConfirm } from "./components/RequestTabs";
import { EnvSelector } from "./components/EnvSelector";
import { RuntimeVarsPanel } from "./components/RuntimeVarsPanel";
import { RequestEditor } from "./components/request/RequestEditor";
import { RequestTabs } from "./components/RequestTabs";
import { QuickOpen } from "./components/QuickOpen";
import { RunTab } from "./components/run/RunTab";
import { SaveRequestDialog } from "./components/SaveRequestDialog";
import { Sidebar } from "./components/Sidebar";
import { SidebarHeader } from "./components/SidebarHeader";
import { SidebarResizer } from "./components/SidebarResizer";
import { useLayout, useSidebarWidth } from "./store/layout";
import { useRuns } from "./store/runs";
import { useTabs } from "./store/tabs";
import { startUp } from "./store/startup";
import { watchVars } from "./store/vars";
import { useWorkspace } from "./store/workspace";

/** Open layers that own Escape. */
const OVERLAY = '[aria-modal="true"], [role="dialog"], [role="menu"], [role="tooltip"]';

export default function App({ hasToken }: { hasToken: boolean }) {
  const { tabs, runTabs, active, requestSave, send, cancel, saveDialog, closeSaveDialog, openRun } = useTabs();
  const { collections, activeEnv, error, clearError, capabilities, openInBrowser, activeCollection } =
    useWorkspace();
  const anyDirty = useTabs((s) => s.tabs.some((t) => s.isDirty(t.key)));
  const [confirmReload, setConfirmReload] = useState(false);
  const [quickOpen, setQuickOpen] = useState(false);
  const startRun = useRuns((s) => s.start);
  const tab = tabs.find((t) => t.key === active) ?? null;
  const runTab = runTabs.find((t) => t.key === active) ?? null;
  const collectionId = tab?.collectionId ?? runTab?.collectionId ?? activeCollection ?? collections[0]?.id ?? null;
  /** "Runs": the collection's newest run in a tab (or an empty one with Previous runs). */
  const showRuns = (id: string) => {
    const { runs, order } = useRuns.getState();
    const latest = [...order].reverse().find((r) => runs[r]?.collectionId === id) ?? null;
    openRun(id, latest, latest ? (runs[latest]?.label ?? "") : (collections.find((c) => c.id === id)?.name ?? "runs"));
  };
  const sidebarWidth = useSidebarWidth();

  useEffect(() => {
    if (!hasToken) return;
    void useLayout.getState().load();
    let stopPersisting: (() => void) | undefined;
    let cancelled = false;
    void startUp(() => cancelled).then((stop) => {
      if (cancelled) stop?.();
      else stopPersisting = stop;
    });
    const stopVars = watchVars();
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
      stopVars();
      stopPersisting?.();
      events.close();
    };
  }, [hasToken]);

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

  // App shortcuts: Cmd/Ctrl+K quick open, Cmd/Ctrl+S save, Cmd/Ctrl+Enter send,
  // Cmd/Ctrl+W close the tab, Esc cancel the running send or run. Monaco handles Cmd/Ctrl+Enter
  // and Cmd/Ctrl+K itself and dispatches "plover:send" / "plover:quick-open" (see CodeEditor).
  // Browsers reserve Cmd/Ctrl+W for closing the window and never deliver it to the
  // page; it works in the desktop app and in browsers that pass it through.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Esc cancels the active tab's send, or the active run tab's running run, unless
      // something on top (a dialog, a menu, a variable card) took it first.
      if (e.key === "Escape") {
        if (e.defaultPrevented || !active || document.querySelector(OVERLAY)) return;
        const { tabs, runTabs } = useTabs.getState();
        if (tabs.find((t) => t.key === active)?.sending) void cancel(active);
        const runId = runTabs.find((t) => t.key === active)?.runId;
        const run = runId ? useRuns.getState().runs[runId] : undefined;
        if (run?.status === "running" && run.source !== "history") useRuns.getState().cancel(run.runId);
        return;
      }
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "k" && !e.shiftKey) {
        e.preventDefault();
        setQuickOpen(true);
      } else if (key === "s") {
        e.preventDefault();
        if (active) requestSave(active);
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
    const onQuickOpen = () => setQuickOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("plover:send", onSend);
    window.addEventListener("plover:quick-open", onQuickOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("plover:send", onSend);
      window.removeEventListener("plover:quick-open", onQuickOpen);
    };
  }, [active, requestSave, send, cancel]);

  if (!hasToken) {
    return (
      <main className="empty">
        <h1>Plover</h1>
        <p>Open the URL printed by <code>plv serve</code> (it contains the session token).</p>
      </main>
    );
  }

  return (
    <div className="app" style={{ "--sidebar-w": `${sidebarWidth}px` } as CSSProperties}>
      <header className="topbar">
        <SidebarHeader />
        <span className="spacer" />
        {capabilities?.desktop && (
          <button type="button" onClick={() => void openInBrowser()}>
            Open in browser
          </button>
        )}
        {collectionId && (
          <button type="button" onClick={() => showRuns(collectionId)}>
            Runs
          </button>
        )}
        <EnvSelector collectionId={collectionId} />
        <RuntimeVarsPanel collectionId={collectionId} />
      </header>
      <QuickOpen key={String(quickOpen)} open={quickOpen} onClose={() => setQuickOpen(false)} />
      {saveDialog && <SaveRequestDialog tabKey={saveDialog} onClose={closeSaveDialog} />}
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
            const runId = startRun(id, item, activeEnv[id] ?? null, label);
            openRun(id, runId, label);
          }}
        />
        <SidebarResizer width={sidebarWidth} />
        <main className="main">
          <RequestTabs />
          {runTab ? (
            <RunTab key={runTab.key} tab={runTab} />
          ) : tab ? (
            <RequestEditor key={tab.key} tab={tab} />
          ) : (
            <EmptyState />
          )}
        </main>
      </div>
    </div>
  );
}
