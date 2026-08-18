import type { RouteName } from "./routes";

/** Every capability the 30-route console had — Plan 123 Slice D.
 *
 *  **This list is the contract.** The route count comes down; this does not.
 *  A screen that vanished in the consolidation would take its functionality
 *  with it, and nobody would notice until a CA went looking for it mid-period,
 *  which is the worst moment to discover a product got smaller.
 *
 *  Named by capability rather than by old route, because several old routes
 *  were the same capability under different nouns and the merge is the point. */
export const CAPABILITIES = [
  // Data
  "upload",
  "mapping",
  "sources",
  "imports",
  "data-quality",
  // Reconcile
  "reconcile",
  "periods",
  "cross-period",
  // Cases
  "register",
  "exceptions",
  "review-queue",
  "case-detail",
  // Intelligence
  "itc-position",
  "working-paper",
  "at-risk",
  "eligibility",
  "supplier-risk",
  "patterns",
  "analytics",
  "agents",
  // Act
  "ims",
  "approvals",
  "follow-ups",
  "suppliers",
  "authority",
  "obligations",
  "deliverables",
  // Settings
  "rules",
  "gstins",
  "users",
  "new-session",
] as const;

export type Capability = (typeof CAPABILITIES)[number];

export interface Section {
  readonly capability: Capability;
  readonly label: string;
  /** Why this capability lives here rather than on its own route. Kept in the
   *  data because "why is X a tab of Y" is the question a later reader asks,
   *  and a merge with no stated reason gets undone. */
  readonly because: string;
}

/** Which capabilities each surviving route hosts, in the order they appear.
 *
 *  The first entry is what opens by default, so it is the route's primary
 *  purpose — a host whose default is a secondary view makes the common case
 *  one click slower, every time. */
export const SECTIONS: Partial<Record<RouteName, readonly Section[]>> = {
  pipeline: [
    { capability: "upload", label: "Upload", because: "the stage's own purpose" },
    {
      capability: "mapping",
      label: "Map columns",
      because: "you map the file you just uploaded; two routes made it two journeys",
    },
    {
      capability: "data-quality",
      label: "Data quality",
      because: "problems found at upload belong beside the upload that found them",
    },
    {
      capability: "sources",
      label: "Sources",
      because: "where files come from is a property of the data stage, not its own screen",
    },
    {
      capability: "imports",
      label: "Import history",
      because: "the same question as Sources, asked backwards in time",
    },
  ],
  reconcile: [
    { capability: "reconcile", label: "This period", because: "the stage's own purpose" },
    {
      capability: "periods",
      label: "All periods",
      because: "choosing a period is navigation within reconciliation, not a destination",
    },
    {
      capability: "cross-period",
      label: "Cross-period",
      because: "the same reconciliation widened; splitting them hid that they answer one question",
    },
  ],
  register: [
    { capability: "register", label: "All invoices", because: "the stage's own purpose" },
    {
      capability: "exceptions",
      label: "Exceptions",
      because: "a filter over the register, which is what it always was",
    },
    {
      capability: "review-queue",
      label: "Review queue",
      because: "the same rows, filtered to what a second pair of eyes owes a decision on",
    },
  ],
  // A single-purpose route still *is* a capability — it simply has one, so it
  // renders no tab strip. Listing it here is what keeps the "every capability
  // has a home" invariant honest rather than special-cased.
  case: [
    { capability: "case-detail", label: "Case", because: "reached by clicking a row, not by nav" },
  ],
  itc: [
    { capability: "itc-position", label: "Position", because: "the stage's own purpose" },
    {
      capability: "at-risk",
      label: "At risk",
      because: "one of the position's four numbers, expanded",
    },
    {
      capability: "eligibility",
      label: "Eligibility",
      because: "why each number landed where it did",
    },
  ],
  // Its own route as well as a link from the ITC position: it is the
  // deliverable a partner reviews and an officer asks about, and burying the
  // thing you hand over inside a tab of something else is how it stops being
  // treated as a document.
  workingpaper: [
    { capability: "working-paper", label: "Working paper", because: "the deliverable" },
  ],
  analytics: [
    { capability: "patterns", label: "Patterns", because: "rings, centrality and orphans" },
    {
      capability: "supplier-risk",
      label: "Supplier risk",
      because: "a per-supplier reading of the same structural signals",
    },
    { capability: "analytics", label: "Trends", because: "the same data over time" },
  ],
  agents: [
    { capability: "agents", label: "Assistants", because: "its own surface" },
  ],
  ims: [
    { capability: "ims", label: "IMS", because: "the stage's own purpose" },
    {
      capability: "approvals",
      label: "Approvals",
      because: "an IMS decision is an approval; two screens made one act look like two",
    },
  ],
  followups: [
    { capability: "follow-ups", label: "Follow-ups", because: "the stage's own purpose" },
    {
      capability: "suppliers",
      label: "Suppliers",
      because: "you chase a supplier, so the directory belongs with the chasing",
    },
  ],
  obligations: [
    { capability: "obligations", label: "Obligations", because: "the stage's own purpose" },
    {
      capability: "authority",
      label: "Authority",
      because: "what the authority says and what it requires of you are one topic",
    },
  ],
  deliverables: [
    { capability: "deliverables", label: "Deliverables", because: "its own surface" },
  ],
  settings: [
    { capability: "rules", label: "Rules", because: "the stage's own purpose" },
    { capability: "gstins", label: "GSTINs", because: "configuration" },
    { capability: "users", label: "Users", because: "configuration" },
    {
      capability: "new-session",
      label: "New session",
      because: "destructive, so it sits behind Settings rather than in the working flow",
    },
  ],
};

/** The capabilities a route hosts. Never `undefined`, so a caller can map over
 *  it without guarding. */
export function sectionsFor(route: RouteName): readonly Section[] {
  return SECTIONS[route] ?? [];
}

/** Whether a route should render a tab strip at all.
 *
 *  A single-capability route renders none — a strip with one tab is chrome
 *  that costs vertical space and tells a reader nothing. */
export function hasTabs(route: RouteName): boolean {
  return sectionsFor(route).length > 1;
}
