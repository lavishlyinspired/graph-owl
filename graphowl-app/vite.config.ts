import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Plan 122a A0: during the migration this app is embedded under `/next/`
  // *alongside* `ui/` at `/` (`crates/graph-owl-ui`'s `router_next()`), so
  // its own asset references must be prefixed accordingly — otherwise the
  // browser requests `/static/*.js` at the root and the *old* console's SPA
  // fallback answers instead. `VITE_BASE` is unset (so `base` defaults to
  // `/`) for local dev; the embed build sets `VITE_BASE=/next/`. A11 removes
  // this — the cutover build always uses the default `/`.
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
