import type { ComponentType } from "react";
import {
  Home,
  Compass,
  BookOpen,
  ShieldCheck,
  BarChart3,
  History,
  Bot,
  Database,
  Plug,
  Package,
  Boxes,
  Server,
  Settings,
  ListChecks,
  CheckCircle2,
  Lock,
  ShieldAlert,
  KeyRound,
} from "lucide-react";
import type { RouteName } from "./routes";
import { ROUTES } from "./routes";
import { strings } from "./strings";

export type NavIcon = ComponentType<{ readonly className?: string }>;

export interface NavItem {
  readonly label: string;
  readonly route: RouteName;
  readonly icon: NavIcon;
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
  { label: "HOME", items: [{ label: "Overview", route: "home", icon: Home }] },
  {
    label: "UNDERSTAND",
    items: [{ label: "Explore", route: "explore", icon: Compass }],
  },
  { label: "VOCABULARY", items: [{ label: "Studio", route: "studio", icon: BookOpen }] },
  {
    label: "GOVERNANCE",
    items: [{ label: "Govern", route: "govern", icon: ShieldCheck }],
  },
  {
    label: "INSIGHT",
    items: [
      { label: "Analytics", route: "analytics", icon: BarChart3 },
      { label: "Agent runs", route: "runs", icon: History },
      { label: "Agents", route: "agents", icon: Bot },
    ],
  },
  {
    label: "INGEST",
    items: [
      { label: "Sources", route: "sources", icon: Database },
      { label: "Connectors", route: "connectors", icon: Plug },
    ],
  },
  {
    label: "PLATFORM",
    items: [
      { label: "Knowledge packs", route: "knowledge", icon: Package },
      { label: "Packs", route: "packs", icon: Boxes },
      { label: "MCP", route: "mcp-tools", icon: Server },
      { label: "Admin", route: "admin", icon: Settings },
      { label: "Tasks", route: "tasks", icon: ListChecks },
      { label: "Quality", route: "quality", icon: CheckCircle2 },
      { label: "Privacy", route: "privacy", icon: Lock },
      { label: "Security", route: "security", icon: ShieldAlert },
      { label: "API Keys", route: "api-keys", icon: KeyRound },
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
