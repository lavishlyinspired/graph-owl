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
 *  point of the five patterns (`00h`) the budget rewards.
 *
 *  Plan 122a A11: `home`, `drift-view` and `mcp-tools` are deliberately not
 *  `overview`/`drift`/`mcp` — those bare slugs collide with real
 *  `graph-owl-server` API paths (`GET /overview`, `GET /drift`,
 *  `POST /mcp`), and axum resolves an exact path match before ever
 *  reaching this app's SPA fallback. Two of the three (`/overview`,
 *  `/drift`) share the API's own GET method, so the collision is silent —
 *  the browser gets the API's JSON, not the console's HTML, with no error
 *  at all. Renaming the console's own route slugs is the contained fix;
 *  the API surface is the versioned, documented, SDK-generating product
 *  surface and does not move for a client's convenience. Nav labels
 *  (`nav.ts`) are unchanged — only the URL segment differs from the label.
 *
 *  The same bug, found again 21 August 2026: `validation`, `contradictions`
 *  and `resolution` collide the same way, but through `vite.config.ts`'s
 *  dev-only proxy rather than axum directly — `/validation/report`,
 *  `/resolution/queue` etc. are real API calls the app makes, so those
 *  proxy prefixes can't be removed, and any full browser navigation
 *  (a hard refresh, a typed URL, a bookmark — not React Router's own
 *  client-side `<Link>`, which never touches the network) to bare
 *  `/validation`, `/contradictions` or `/resolution` gets forwarded
 *  straight to `graph-owl-server` on :8080 instead of reaching Vite's SPA
 *  fallback, and renders whatever stale build that origin happens to be
 *  serving. Renamed to `validation-view`/`contradictions-view`/
 *  `resolution-view`, matching `drift-view`'s existing precedent exactly
 *  — moot for routing purposes since, per the next paragraph, none of the
 *  five are routes any more, but the collision-avoidant names stayed
 *  since the files themselves (and their component names) did not move.
 *
 *  21 August 2026: `validation-view`, `contradictions-view`,
 *  `resolution-view`, `drift-view` and `governance` folded into one
 *  `govern` route with five tabs (`routes/govern.tsx`), the same way
 *  Vocabulary Studio absorbs its own nine tabs under `studio` — one nav
 *  slot, not five. The five component files are unchanged and still
 *  exported the same way; only their standalone routes are gone. */

export const ROUTES = [
  "home",
  "explore",
  "entity",
  "knowledge",
  "govern",
  "sources",
  "connectors",
  "pipeline",
  "studio",
  "analytics",
  "runs",
  "packs",
  "agents",
  "mcp-tools",
  "admin",
  "tasks",
  "quality",
  "privacy",
  "security",
  "api-keys",
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
