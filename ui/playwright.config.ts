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
  retries: 0,
  reporter: "list",
  use: {
    baseURL: process.env.GRAPH_OWL_BASE_URL ?? "http://127.0.0.1:8099",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
