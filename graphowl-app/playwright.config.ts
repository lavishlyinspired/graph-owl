import { defineConfig, devices } from "@playwright/test";

/** Plan 122a A11. Same reasoning as `_archived/ui/playwright.config.ts`: no
 *  `webServer` here, deliberately — the journey needs a real
 *  `graph-owl-server` bound to a real Postgres, which is Rust process +
 *  Docker setup Playwright's own server-launcher does not model.
 *  `scripts/verify-first-run-journey.sh` stands both up and passes the URL
 *  via `GRAPH_OWL_BASE_URL`, the same contract the archived script used. */
export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: process.env.GRAPH_OWL_BASE_URL ?? "http://127.0.0.1:8099",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
