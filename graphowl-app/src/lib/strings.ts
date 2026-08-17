/** Every user-visible literal lives here, not inline in JSX — enforced by
 *  `eslint-rules/no-raw-jsx-text.mjs` (ported from `ui/`, Epic 39 Slice E
 *  decision 7: "No user-visible literal in a component," so i18n never
 *  needs a retrofit). */
export const strings = {
  brand: "GRAPHOWL",
  searchPlaceholder: "Search or ask GraphOWL…",
  searchShortcut: "⌘K",
  themeDay: "DAY",
  themeNight: "NIGHT",
  inboxTitle: "Waiting on you",
  inboxSubtitle: "agents queue here; nothing applies itself",
  inboxFooter:
    "Automatic agents never reach this list — they only read. Anything that would change the graph lands here first.",
  inboxEmpty: "Nothing waiting on you right now.",
  approve: "Approve",
  reject: "Reject",
  close: "Close",
  collapseNav: "COLLAPSE",
  workspaceHeading: "WORKSPACE · ISOLATED GRAPH + PACKS",
  workspaceFooter: "Switching changes the graph, the installed packs and the audit trail. Nothing is shared between workspaces.",
  workspaceSingleNote: "Multi-workspace isolation is not yet available — every session uses this one workspace.",
  searchIcon: "⌕",
  inboxIcon: "⚑",
  closeIcon: "×",
  inboxReasonPlaceholder: "Why? (required)",
  inboxReasonConfirm: "Confirm reject",
  inboxReasonCancel: "Cancel",
  inboxActionFailed: "That didn't go through — try again.",
  searchNoResults: "No matches.",
  searchAsset: "ASSET",
  searchGlossaryTerm: "GLOSSARY TERM",
  searchBusinessMetric: "BUSINESS METRIC",
} as const;
