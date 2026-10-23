import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Dev loop: run `plv serve --no-open --port 4000 --dev-origin http://localhost:5173`
// and open http://localhost:5173/#token=<token from the serve output>.
const api = process.env.PLOVER_API ?? "http://127.0.0.1:4000";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: api, ws: true },
    },
  },
  test: {
    environment: "jsdom",
  },
});
