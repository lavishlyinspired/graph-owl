import type { RouteName } from "./routes";

export interface NavItem {
  readonly label: string;
  readonly route: RouteName;
}

export interface NavGroup {
  readonly label: string;
  readonly items: readonly NavItem[];
}

/** Verbatim group structure from the delivered mockup's own `nav` array
 *  (`Reco Now.dc.html`'s `renderVals()`). */
export const NAV: readonly NavGroup[] = [
  { label: "HOME", items: [{ label: "Dashboard", route: "home" }] },
  {
    label: "RECONCILE",
    items: [
      { label: "Upload & map", route: "pipeline" },
      { label: "Periods", route: "periods" },
      { label: "Register", route: "register" },
      { label: "Exceptions", route: "exceptions" },
      { label: "Case detail", route: "case" },
      { label: "Cross-period", route: "crossperiod" },
    ],
  },
  {
    label: "ITC",
    items: [
      { label: "ITC position", route: "itc" },
      { label: "At risk", route: "atrisk" },
      { label: "Eligibility", route: "eligibility" },
    ],
  },
  {
    label: "COMPLIANCE",
    items: [
      { label: "Authority", route: "authority" },
      { label: "Obligations", route: "obligations" },
    ],
  },
  {
    label: "SUPPLIERS",
    items: [
      { label: "Suppliers", route: "suppliers" },
      { label: "Supplier risk", route: "risk" },
      { label: "Follow-ups", route: "followups" },
    ],
  },
  {
    label: "OPERATE",
    items: [
      { label: "Review queue", route: "queue" },
      { label: "IMS", route: "ims" },
      { label: "Approvals", route: "approvals" },
      { label: "Assistants", route: "agents" },
    ],
  },
  {
    label: "DELIVER",
    items: [
      { label: "Deliverables", route: "deliverables" },
      { label: "Analytics", route: "analytics" },
    ],
  },
  {
    label: "DATA",
    items: [
      { label: "Imports", route: "imports" },
      { label: "Sources", route: "datasources" },
      { label: "Mappings", route: "mappings" },
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
