/** Plan 122b B1: the 28 destinations from the delivered mockup's own `nav`
 *  array (`Reco Now.dc.html`'s `renderVals().nav`), read off verbatim —
 *  same route-budget discipline `graphowl-app/src/lib/routes.ts` already
 *  established for the console.
 *
 *  6 are bespoke screens (`BESPOKE_ROUTES` below); the other 22 render
 *  through one shared, config-driven template (the mockup's own
 *  `isGeneric: !bespoke[cur]` — a single component driven by a per-screen
 *  `screens()` config, not 22 separate page components). */
export const ROUTES = [
  "home",
  "pipeline",
  "reconcile",
  "periods",
  "register",
  "exceptions",
  "case",
  "crossperiod",
  "itc",
  "atrisk",
  "eligibility",
  "authority",
  "obligations",
  "suppliers",
  "risk",
  "followups",
  "queue",
  "ims",
  "approvals",
  "agents",
  "deliverables",
  "analytics",
  "imports",
  "datasources",
  "mappings",
  "rules",
  "gstins",
  "users",
  "reset",
] as const;

export type RouteName = (typeof ROUTES)[number];

export const BESPOKE_ROUTES: readonly RouteName[] = ["home", "pipeline", "register", "case", "agents", "analytics"];

export const MAX_ROUTES = 30;

export interface RouteBudgetResult {
  readonly count: number;
  readonly max: number;
  readonly ok: boolean;
}

export function checkRouteBudget(routes: readonly string[], max: number = MAX_ROUTES): RouteBudgetResult {
  return { count: routes.length, max, ok: routes.length <= max };
}
