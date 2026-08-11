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
  "explore",
  "governance",
  "workbench",
  "vocabulary",
  "review",
  "obligations",
  "connectors",
  "admin",
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
