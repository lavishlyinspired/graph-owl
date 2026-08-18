import type { RouteName } from "./routes";
import { ROUTES } from "./routes";
import { strings } from "./strings";

export interface NavItem {
  readonly label: string;
  readonly route: RouteName;
}

export interface NavGroup {
  readonly label: string;
  readonly items: readonly NavItem[];
}

/** Verbatim group structure from the delivered mockup's `nav` array
 *  (`GraphOWL Console.dc.html`). "Source mapping" (`pipeline`) is
 *  deliberately absent — it is reached from Sources, not a nav item of its
 *  own, matching `plans/122a-graphowl-app.md` §2's route-budget accounting. */
export const NAV: readonly NavGroup[] = [
  { label: "HOME", items: [{ label: "Overview", route: "home" }] },
  {
    label: "UNDERSTAND",
    items: [
      { label: "Explore", route: "explore" },
      { label: "Entity", route: "entity" },
      { label: "Knowledge", route: "knowledge" },
    ],
  },
  {
    label: "TRACE",
    items: [
      { label: "Lineage", route: "lineage-view" },
      { label: "Paths", route: "paths" },
      { label: "History", route: "history" },
      { label: "Evidence", route: "evidence" },
    ],
  },
  {
    label: "GOVERN",
    items: [
      { label: "Validation", route: "validation" },
      { label: "Contradictions", route: "contradictions" },
      { label: "Resolution", route: "resolution" },
      { label: "Drift", route: "drift-view" },
      { label: "Governance", route: "governance" },
    ],
  },
  {
    label: "INGEST",
    items: [
      { label: "Sources", route: "sources" },
      { label: "Connectors", route: "connectors" },
    ],
  },
  { label: "VOCABULARY", items: [{ label: "Studio", route: "studio" }] },
  {
    label: "INSIGHT",
    items: [
      { label: "Analytics", route: "analytics" },
      { label: "Agent runs", route: "runs" },
    ],
  },
  {
    label: "PLATFORM",
    items: [
      { label: "Workbench", route: "workbench" },
      { label: "Packs", route: "packs" },
      { label: "Agents", route: "agents" },
      { label: "MCP", route: "mcp-tools" },
      { label: "Admin", route: "admin" },
    ],
  },
];

const NAV_LABEL_BY_ROUTE = new Map<string, string>(
  NAV.flatMap((group) => group.items.map((item) => [item.route, item.label] as const)),
);

/** Plan 122a A11: axe's `page-has-heading-one` needs a real h1 naming the
 *  current screen — this console has no on-screen page title by design (an
 *  instrument panel, not a document), so `AppShell` renders one visually
 *  hidden, sourced from here. Pure so the mapping can be tested without a
 *  router. Falls back to the brand name for a route reached contextually
 *  (`pipeline`) rather than through `NAV` — see `nav.test.ts`'s "covers
 *  every route" case for the one route that takes this path. */
export function pageTitleForPath(pathname: string): string {
  const segment = pathname.split("?")[0]?.split("/").filter(Boolean)[0] ?? ROUTES[0];
  return NAV_LABEL_BY_ROUTE.get(segment) ?? strings.brand;
}
