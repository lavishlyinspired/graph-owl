import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Plan 122a A0 embedded this app under `/next/` *alongside* `ui/` at `/`
  // during the migration, so `VITE_BASE=/next/` prefixed its own asset
  // references. A11 promoted it to serve `/` directly and archived `ui/`
  // — `base` is always the default `/` now; `VITE_BASE` is kept as a no-op
  // override rather than removed outright, in case a future embed prefix
  // is ever needed again.
  base: process.env.VITE_BASE ?? "/",
  // NOT "assets": /assets/{id} is an API resource namespace, and a bundle at
  // /assets/index-*.js is claimed by that route before the SPA fallback sees
  // it — the app then cannot load its own JavaScript (ui/vite.config.ts's
  // own note, still true here).
  build: { outDir: "dist", assetsDir: "static", emptyOutDir: true },
  // Dev-only. In production the SPA is embedded in the binary and served from
  // the same origin — 00f-ui-architecture.md.
  server: { proxy: { "/api": { target: "http://localhost:8080", rewrite: (p) => p.replace(/^\/api/, "") } } },
  test: { exclude: ["**/node_modules/**", "**/dist/**", "tests/**"], environment: "jsdom" },
});
