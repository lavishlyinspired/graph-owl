import type { RouteName } from "./routes";

export interface NavItem {
  readonly label: string;
  readonly route: RouteName;
}

export interface NavGroup {
  readonly label: string;
  readonly items: readonly NavItem[];
}

/** **The five-stage shape — Plan 123 Slice D.**
 *
 *  Replaces the delivered mockup's own eight-group `nav` array, which grouped
 *  by *noun*: ITC, SUPPLIERS, COMPLIANCE, DELIVER, DATA. That made a reviewer
 *  know which category held the screen they wanted before they could reach it,
 *  and it put screens that are used together (Reconcile and Exceptions, ITC
 *  position and the 3B working paper) in different places.
 *
 *  The stages name **what you are doing**, in the order a period is actually
 *  worked:
 *
 *    Data       get it in and mapped
 *    Reconcile  match the two sides
 *    Cases      work what did not match
 *    Intelligence  understand the position the cases leave you in
 *    Act        do something about it
 *
 *  Each stage depends only on the ones before it, which `nav.test.ts` asserts
 *  — a nav in another order teaches the wrong sequence to whoever learns the
 *  product from it.
 *
 *  **No screen was removed in the regrouping.** The plan's "stop maintaining
 *  28" is about the information architecture, not about deleting
 *  functionality: which screens should cease to exist is a product decision,
 *  and every one of them is still reachable here. */
export const NAV: readonly NavGroup[] = [
  { label: "HOME", items: [{ label: "Dashboard", route: "home" }] },
  {
    label: "DATA",
    items: [
      { label: "Upload & map", route: "pipeline" },
      { label: "Imports", route: "imports" },
      { label: "Sources", route: "datasources" },
      { label: "Mappings", route: "mappings" },
    ],
  },
  {
    label: "RECONCILE",
    items: [
      { label: "Reconcile", route: "reconcile" },
      { label: "Periods", route: "periods" },
      { label: "Cross-period", route: "crossperiod" },
    ],
  },
  {
    label: "CASES",
    items: [
      { label: "Register", route: "register" },
      { label: "Exceptions", route: "exceptions" },
      { label: "Case detail", route: "case" },
      { label: "Review queue", route: "queue" },
    ],
  },
  {
    label: "INTELLIGENCE",
    items: [
      { label: "ITC position", route: "itc" },
      { label: "GSTR-3B working paper", route: "workingpaper" },
      { label: "At risk", route: "atrisk" },
      { label: "Eligibility", route: "eligibility" },
      { label: "Supplier risk", route: "risk" },
      { label: "Analytics", route: "analytics" },
      { label: "Assistants", route: "agents" },
    ],
  },
  {
    label: "ACT",
    items: [
      { label: "IMS", route: "ims" },
      { label: "Approvals", route: "approvals" },
      { label: "Follow-ups", route: "followups" },
      { label: "Suppliers", route: "suppliers" },
      { label: "Authority", route: "authority" },
      { label: "Obligations", route: "obligations" },
      { label: "Deliverables", route: "deliverables" },
    ],
  },
  {
    label: "SETTINGS",
    items: [
      { label: "Rules", route: "rules" },
      { label: "GSTINs", route: "gstins" },
      { label: "Users", route: "users" },
      { label: "New session", route: "reset" },
    ],
  },
];
