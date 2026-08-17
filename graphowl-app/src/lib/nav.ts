import type { RouteName } from "./routes";

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
  { label: "HOME", items: [{ label: "Overview", route: "overview" }] },
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
      { label: "Lineage", route: "lineage" },
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
      { label: "Drift", route: "drift" },
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
      { label: "MCP", route: "mcp" },
      { label: "Admin", route: "admin" },
    ],
  },
];
