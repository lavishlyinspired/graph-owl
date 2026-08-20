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
  // In production graph-owl serves this SPA itself, so `lib/api.ts` uses an
  // empty API_BASE and calls bare paths like `/overview`. The dev server used
  // to proxy only `/api`, which nothing calls, and to :8081, where nothing
  // listens — so every request fell through to vite, came back as the SPA's
  // own index.html, and failed to parse as JSON. The console reported "Could
  // not load the overview" and was right.
  //
  // Proxying graph-owl's real top-level prefixes reproduces the
  // same-origin arrangement the app is written for. The list is graph-owl's
  // own OpenAPI top-level paths; anything not here is a vite asset or an HMR
  // route and must stay local.
  server: {
    port: 5174,
    proxy: {
      ...Object.fromEntries(
        [
          "admin", "agents", "alignments", "assets", "auth", "business-metrics",
          "change-proposals", "connectors", "context", "contradictions",
          "custom-properties", "cypher", "drift", "extraction", "findings",
          "glossaries", "glossary-terms", "graph", "health", "inbox", "ingest",
          "lineage", "mcp", "me", "memories", "merges", "metrics", "namespaces",
          "ontology", "ontology-editor", "ontology-packs", "overview", "packs", "policies", "posts",
          "proposals", "ready", "reasoning", "recertification-queue",
          "relationships", "resolution", "search", "sparql", "tables", "teams",
          "threads", "users", "validation", "webhooks", "openapi.json",
        ].map((p) => [`^/${p}(/|$|\\?)`, { target: "http://localhost:8080", changeOrigin: true }]),
      ),
      // The "ask GraphOWL" bar — a separate, out-of-process Python service
      // (`examples/gst-reconcile/ask_server.py`), never graph-owl-server
      // itself: `plans/00j-language-boundaries.md` keeps agent/LLM
      // orchestration outside the Rust binary on purpose. Run it with
      // `python3 examples/gst-reconcile/ask_server.py` before this route
      // is reachable — see `graphowl-app/plans/ask-graphowl.md`.
      "^/ask(/|$|\\?)": { target: "http://localhost:8090", changeOrigin: true },
    },
  },
  test: { exclude: ["**/node_modules/**", "**/dist/**", "tests/**"], environment: "jsdom" },
});
