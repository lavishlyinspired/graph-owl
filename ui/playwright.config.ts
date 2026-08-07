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
  // **`first-run.spec.ts` must run first, and that is an ordering
  // convention this config does not enforce — it relies on every spec
  // file sorting after it alphabetically.** `workers: 1` makes files run
  // sequentially in discovery order (alphabetical here), and
  // `first-run.spec.ts`'s own test asserts the *empty*-database state; any
  // spec that runs before it and creates real data breaks that assertion
  // with a state-dependent axe violation, not an obvious "wrong data"
  // failure. Found with Epic 42 Slice F's own spec file, originally named
  // `agent-activity.spec.ts` (sorts before "first-run" — 'a' < 'f') and
  // renamed to `governance-agent-activity.spec.ts` to fix it. Name a new
  // spec file so it sorts after "first-run" — anything starting with a
  // letter from 'g' onward is safe, matching every other file here.
  retries: 0,
  reporter: "list",
  use: {
    baseURL: process.env.GRAPH_OWL_BASE_URL ?? "http://127.0.0.1:8099",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
