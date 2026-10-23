import { Component, type ReactNode } from "react";
import { api } from "../api/instance";

/**
 * Shows a render crash instead of a blank window, and reports it as the startup error
 * (`POST /api/client-ready`) so the desktop smoke test prints it.
 */
export class StartupBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };

  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? e.message : String(e) };
  }

  componentDidCatch(e: unknown) {
    try {
      void api()
        .clientReady(`render failed: ${e instanceof Error ? e.message : String(e)}`)
        .catch(() => undefined);
    } catch {
      // No session token: nothing to report to.
    }
  }

  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <main className="empty" role="alert">
        <h1>Plover could not start</h1>
        <p>{this.state.error}</p>
        <button type="button" onClick={() => window.location.reload()}>
          Reload
        </button>
      </main>
    );
  }
}
