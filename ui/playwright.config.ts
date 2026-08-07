import { defineConfig, devices } from "@playwright/test";

/** Epic 39 Slice F. No `webServer` here, deliberately: the journey needs a
 *  real `graph-owl-server` bound to a real (empty) Postgres, which is Rust
 *  process + Docker container setup Playwright's own server-launcher does
 *  not model. `scripts/verify-first-run-journey.sh` stands both up, the same
 *  way `scripts/verify-generated-client.sh` already does for the generated
 *  API client, and passes the URL via `GRAPH_OWL_BASE_URL`. */
export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  // `fullyParallel: false` only orders tests *within* a file — Playwright
  // still schedules different files onto separate worker processes by
  // default (here, up to ~9 on an 18-core machine), and every file in
  // this directory drives the identical shared server and database. Found
  // running all four spec files together for the first time (Epic 42
  // Slice D, the fourth file): `first-run.spec.ts` and `review-queue.spec.ts`
  // failed intermittently only in that combination, never alone — two
  // files creating and reading state concurrently against one backend
  // that assumes it is the only writer. `workers: 1` is what actually
  // makes the whole suite sequential, which `fullyParallel: false` reads
  // as though it already guaranteed.
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: process.env.GRAPH_OWL_BASE_URL ?? "http://127.0.0.1:8099",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
