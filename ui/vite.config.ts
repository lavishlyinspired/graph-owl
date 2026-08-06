import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
    // NOT "assets": /assets/{id} is an API resource namespace, and a bundle at
  // /assets/index-*.js is claimed by that route before the SPA fallback sees
  // it — the app then cannot load its own JavaScript.
  build: { outDir: "dist", assetsDir: "static", emptyOutDir: true },
  // Dev-only. In production the SPA is embedded in the binary and served from
  // the same origin, so there is no proxy and no CORS — 00f-ui-architecture.md.
  server: { proxy: { "/api": { target: "http://localhost:8080", rewrite: (p) => p.replace(/^\/api/, "") } } },
  // `tests/` is Playwright's directory (`*.spec.ts`), which Vitest's default
  // include glob would otherwise also pick up and run with the wrong test
  // framework's globals against no real server at all.
  test: { exclude: ["**/node_modules/**", "**/dist/**", "tests/**"] },
});
