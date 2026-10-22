import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createApiClient } from "./api/client";
import { setApi } from "./api/instance";
import { takeTokenFromLocation } from "./api/token";
import App from "./App";
import "./styles.css";

const token = takeTokenFromLocation(window.location, window.sessionStorage);
if (token) setApi(createApiClient("", token));

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App hasToken={!!token} />
  </StrictMode>,
);
