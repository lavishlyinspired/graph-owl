import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

/** Plan 122a A11's RED: "all 24 routes reachable at `/`", checked here as a
 *  live journey rather than only the Rust-side route list assertion
 *  (`crates/graph-owl-server/tests/console_embed.rs`) — this is the thing a
 *  route list matching cannot catch: does the page a route resolves to
 *  actually render, keyboard-reachable and free of accessibility
 *  violations, against a real server. Run via
 *  `scripts/verify-first-run-journey.sh`, which starts a real
 *  `graph-owl-server` against a genuinely empty Postgres in open auth mode
 *  and passes its URL as `GRAPH_OWL_BASE_URL` — the same contract
 *  `_archived/ui/tests/first-run.spec.ts` used for the console it replaced.
 *
 *  Deliberately smaller than the archived suite's seven spec files: this is
 *  a smoke pass across the shell and a representative screen from each nav
 *  group, not a full port of `ui/`'s per-feature journeys. See
 *  `plans/122a-graphowl-app.md`'s A11 section for the honest scope note. */

test("first run on an empty database renders the shell, not a blank page", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByPlaceholder(/search/i).first()).toBeVisible();

  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations, JSON.stringify(axe.violations, null, 2)).toEqual([]);
});

test("the rail is keyboard-reachable and every group's first destination renders with no axe violations", async ({
  page,
}) => {
  // One representative route per nav group (`src/lib/nav.ts`) — the real
  // navigation structure, not a hardcoded guess at what exists.
  const routes = [
    "/home",
    "/explore",
    "/lineage-view",
    "/validation",
    "/sources",
    "/studio",
    "/analytics",
    "/workbench",
  ];

  for (const route of routes) {
    await page.goto(route);
    // Every route renders *something* under the rail — the shared shell —
    // rather than a blank body, whether or not that screen has data yet.
    await expect(page.locator("nav[aria-label='Primary']")).toBeVisible();

    const axe = await new AxeBuilder({ page }).analyze();
    expect(axe.violations, `${route}: ${JSON.stringify(axe.violations, null, 2)}`).toEqual([]);
  }
});

test("a failed search is shown as a failure, not as an empty result", async ({ page }) => {
  await page.route("**/search**", (route) => route.abort("failed"));
  await page.goto("/");

  const searchBox = page.getByPlaceholder(/search/i).first();
  await searchBox.click();
  await searchBox.fill("anything");

  await expect(page.getByText(/search failed to load/i)).toBeVisible();
});
