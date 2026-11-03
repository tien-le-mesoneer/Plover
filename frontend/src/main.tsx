import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createApiClient, exchangeLaunchCode, openSession } from "./api/client";
import { setApi } from "./api/instance";
import { resolveSessionToken } from "./api/token";
import App from "./App";
import { StartupBoundary } from "./components/StartupBoundary";
import { startTheme } from "./lib/appTheme";
import "./assets/fonts/fonts.css";
import "./styles.css";

const sessionToken = () =>
  resolveSessionToken({
    loc: window.location,
    storage: window.sessionStorage,
    exchange: (code) => exchangeLaunchCode("", code),
    open: () => openSession(""),
  });

// Resolve the OS scheme before anything renders; the saved choice is applied once the layout loads.
startTheme();

void sessionToken().then((token) => {
  if (token) setApi(createApiClient("", token));
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <StartupBoundary>
        <App hasToken={!!token} />
      </StartupBoundary>
    </StrictMode>,
  );
});
