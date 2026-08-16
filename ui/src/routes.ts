/** Epic 42 Slice F: the console route budget (`00h-ui-design-system.md`,
 *  "Route budget" — **≤ 30 routes, CI-asserted**). This app has no router:
 *  navigation is a manual `?section=` query param plus `history.replaceState`
 *  (`features/deepLink.ts`). A "route" here is therefore defined as **the set
 *  of distinct top-level `section` values `App.tsx`'s own switch recognizes**
 *  — the honest equivalent of what a router's route table would list if one
 *  existed, and the same thing a user can bookmark or deep-link to.
 *
 *  What is deliberately *not* counted as a separate route: admin's own tabs
 *  (`adminTab=`), the vocabulary/queue pickers, and the asset search-and-detail
 *  view reached with no `section` set at all. Each of those is one route
 *  absorbing many features through a config or a tab, which is the whole
 *  point of the five patterns the budget exists to reward — counting them
 *  separately would double-count the exact thing the budget is designed to
 *  measure.
 *
 *  `ROUTES` must be kept in sync with `App.tsx`'s `section === "..."` switch —
 *  `routes.structural.test.ts` greps the real source and fails the build if
 *  they drift apart. */

export const ROUTES = [
  "overview",
  // Plan 108 Slice 7. A route of its own rather than a tab under Admin: for
  // the person closing a period this *is* the product, and it was previously
  // reachable only by uploading through an admin table, clicking a button in
  // one of its rows, and reading the result in a third section.
  "reconciliation",
  "explore",
  "governance",
  // Plan 120 Slice H: no longer a route of its own — Workbench (SPARQL +
  // Cypher) merged into a tab inside "ontology-builder", the same domain
  // the queries are usually written to explore, rather than a separate
  // destination a reader has to navigate away to reach.
  "vocabulary",
  "review",
  "obligations",
  // Plan 107 Slice 4 (`plans/107-filing-period.md`): the console surface
  // for period-list/period-summary/period-diff — "obligation-calendar-
  // shaped" per the plan's own words, so it gets the identical treatment
  // as "obligations" above: a route of its own, not a tab, and named
  // explicitly in App.tsx's deep-link whitelist from the start (that
  // list's own comment records "obligations" being missing there once
  // as a real, previously-shipped bug).
  "filing-periods",
  "connectors",
  "admin",
  "agent",
  "ontology-builder",
] as const;

export type Route = (typeof ROUTES)[number];

export const MAX_ROUTES = 30;

export interface RouteBudgetResult {
  readonly count: number;
  readonly max: number;
  readonly ok: boolean;
}

/** Pure so it can be proven to fail before it is ever pointed at the real
 *  route list — the plan's own named RED test: "the route-budget check must
 *  **fail** the build when a fixture exceeds 30." */
export function checkRouteBudget(
  routes: readonly string[],
  max: number = MAX_ROUTES,
): RouteBudgetResult {
  return { count: routes.length, max, ok: routes.length <= max };
}
