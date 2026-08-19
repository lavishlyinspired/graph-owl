/** **13 routes, down from 30 — Plan 123 Slice D.**
 *
 *  The mockup's own `nav` array had 30 destinations, most of which were the
 *  same capability under a different noun: Exceptions was a filter over the
 *  Register, Approvals was an IMS decision, Sources and Imports asked one
 *  question in two directions. Maintaining them separately meant a change to
 *  the register's table had to be made three times.
 *
 *  **Nothing was removed.** Every capability the 30 routes carried is listed
 *  in `sections.ts` and hosted on one of these, as a named section — and
 *  `sections.test.ts` fails if any is left without a home. The consolidation
 *  is an information-architecture change; a product that got smaller would be
 *  a different decision and not one to make silently. */
export const ROUTES = [
  "home",
  "pipeline",
  "reconcile",
  "register",
  "itc",
  "workingpaper",
  "analytics",
  "agents",
  "ims",
  "followups",
  "obligations",
  "deliverables",
  "settings",
] as const;

export type RouteName = (typeof ROUTES)[number];

/** Screens with their own layout rather than the config-driven template. */
export const BESPOKE_ROUTES: readonly RouteName[] = [
  "home",
  "pipeline",
  "reconcile",
  "register",
  "workingpaper",
  "agents",
  "analytics",
];

/** Down from 30. The budget is the point of the slice, so it is enforced. */
export const MAX_ROUTES = 15;

export interface RouteBudgetResult {
  readonly count: number;
  readonly max: number;
  readonly ok: boolean;
}

export function checkRouteBudget(routes: readonly string[], max: number = MAX_ROUTES): RouteBudgetResult {
  return { count: routes.length, max, ok: routes.length <= max };
}
