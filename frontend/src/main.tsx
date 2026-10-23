import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createApiClient, exchangeLaunchCode } from "./api/client";
import { setApi } from "./api/instance";
import { storeToken, takeLaunchCode, takeTokenFromLocation } from "./api/token";
import App from "./App";
import { StartupBoundary } from "./components/StartupBoundary";
import "./styles.css";

/** A launch code (`#code=`) is traded for the token; `#token=` or this tab's stored token also work. */
async function sessionToken(): Promise<string | null> {
  const code = takeLaunchCode(window.location);
  if (code) {
    try {
      const token = await exchangeLaunchCode("", code);
      storeToken(token, window.sessionStorage);
      return token;
    } catch {
      // Used or expired code: fall back to a token this tab already has, if any.
    }
  }
  return takeTokenFromLocation(window.location, window.sessionStorage);
}

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
