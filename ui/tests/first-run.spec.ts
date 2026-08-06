import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Epic 39 Slice F's RED: the empty-database journey. "It is the first thing
 *  every evaluator sees and the last thing anyone tests, and a console that
 *  opens on a wall of empty panels loses the evaluation in the first thirty
 *  seconds." Run via `scripts/verify-first-run-journey.sh`, which starts a
 *  real `graph-owl-server` against a genuinely empty Postgres in open auth
 *  mode (no OIDC to drive through Playwright) and passes its URL as
 *  `GRAPH_OWL_BASE_URL`. */

test("first run on an empty database, through to viewing the first ingested asset", async ({
  page,
  baseURL,
}) => {
  await page.goto("/");

  // Empty state: the AC is "renders the designed first-run state with a
  // next action, not '0 results'" — search must not even be reachable as
  // "0 results" because there is nothing to have searched yet. The shell
  // itself has no HTML heading (the wordmark is an inline SVG), so the
  // search box is the real signal the shell rendered rather than a blank
  // or crashed page.
  await expect(page.getByText(/0 results/i)).toHaveCount(0);
  await expect(page.getByPlaceholder(/search/i).first()).toBeVisible();

  const emptyAxe = await new AxeBuilder({ page }).analyze();
  expect(emptyAxe.violations, JSON.stringify(emptyAxe.violations, null, 2)).toEqual([]);

  // "Connecting a source" end to end (OAuth-less connector run, arbitrary
  // adapter selection) is exercised by this project's Rust connector
  // integration tests; what this journey verifies is the console's own
  // empty -> populated transition, so the one asset is created directly
  // against the real API the console itself calls — the same shape a
  // connector's first sync would produce.
  //
  // `POST /tables` (a bare table with no parent chain) does not work here —
  // confirmed by hand against a real server: it writes a row but never
  // becomes searchable, because a `table` is not a root of the hierarchy.
  // `POST /assets` with `kind: "service"` and `parentId: null` is the
  // documented cheap fixture for "one real, searchable root asset" (see
  // CLAUDE.md's Epic 31 gotcha, the same shape recurring here).
  const created = await page.request.post(`${baseURL}/assets`, {
    data: {
      kind: "service",
      name: "first_run_journey_table",
      parentId: null,
      description: "Created by the Epic 39 Slice F first-run journey test.",
    },
  });
  expect(created.ok()).toBe(true);

  // Keyboard-only from here: focus the search box, type, arrow down to the
  // (only) result, Enter to open it — `00f`'s non-negotiable keyboard
  // requirement, exercised as part of the same journey rather than a
  // separate pass that could silently drift from what users actually do.
  const searchBox = page.getByPlaceholder(/search/i).first();
  await searchBox.click();
  await searchBox.fill("first_run_journey_table");
  // "first_run_journey_table" legitimately appears three times in the
  // result row alone (result-count heading, name cell, FQN cell) — the
  // count heading is the one unambiguous signal that the row arrived.
  await expect(page.getByRole("heading", { name: /1 result for/i })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");

  await expect(page.getByRole("heading", { name: "first_run_journey_table" })).toBeVisible();

  const entityAxe = await new AxeBuilder({ page }).analyze();
  expect(entityAxe.violations, JSON.stringify(entityAxe.violations, null, 2)).toEqual([]);
});

test("a failed search is shown as a failure, not as an empty result", async ({ page }) => {
  // The RED this closes: the search catch block used to render `results: []`
  // on any error — identical to a genuine "nothing matched" — so a backend
  // outage looked exactly like an empty catalog. Intercepting the request is
  // the only way to prove the distinction exists, short of taking the real
  // server down mid-test.
  await page.route("**/search**", (route) => route.abort("failed"));
  await page.goto("/");

  const searchBox = page.getByPlaceholder(/search/i).first();
  await searchBox.click();
  await searchBox.fill("anything");

  await expect(page.getByText(/search failed to load/i)).toBeVisible();
  await expect(page.getByText("Nothing matched")).toHaveCount(0);
});
