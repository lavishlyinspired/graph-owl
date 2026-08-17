/** Plan 122a A0: the console route budget (`00h-ui-design-system.md`,
 *  "Route budget" — **≤ 30 routes, CI-asserted**). Unlike `ui/`'s
 *  `?section=` switch, this app has a real router (`react-router-dom`), so a
 *  "route" here is the literal set of paths registered with it — checked
 *  directly against the router config by `routes.structural.test.ts`,
 *  rather than grepped out of a component's source text.
 *
 *  Not counted as routes of their own, matching `ui/`'s original budget
 *  reasoning: Vocabulary Studio's 8 tabs, Admin's tabs, and any
 *  drawer/detail view reached without a distinct path. Each of those is one
 *  route absorbing many features through a config or a tab — the whole
 *  point of the five patterns (`00h`) the budget rewards. */

export const ROUTES = [
  "overview",
  "explore",
  "entity",
  "knowledge",
  "lineage",
  "paths",
  "history",
  "evidence",
  "validation",
  "contradictions",
  "resolution",
  "drift",
  "governance",
  "sources",
  "connectors",
  "pipeline",
  "studio",
  "analytics",
  "runs",
  "workbench",
  "packs",
  "agents",
  "mcp",
  "admin",
] as const;

export type RouteName = (typeof ROUTES)[number];

export const MAX_ROUTES = 30;

export interface RouteBudgetResult {
  readonly count: number;
  readonly max: number;
  readonly ok: boolean;
}

/** Pure so it can be proven to fail before it is ever pointed at the real
 *  route list — same discipline as `ui/src/routes.ts`'s own named RED test. */
export function checkRouteBudget(routes: readonly string[], max: number = MAX_ROUTES): RouteBudgetResult {
  return { count: routes.length, max, ok: routes.length <= max };
}
