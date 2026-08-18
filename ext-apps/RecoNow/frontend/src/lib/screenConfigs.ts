/** Per-screen config for the 22 non-bespoke screens.
 *  Data lifted from the delivered mockup's own `screens()` function
 *  (`Reco Now.dc.html`), `viz()` function, `copilot()` function,
 *  and `rowActs()` function — read straight off the mockup's own
 *  data rather than approximated. */

export interface KpiItem {
  readonly label: string;
  readonly value: string;
  readonly sub: string;
  readonly color: string;
}

export interface CellData {
  readonly t: string;
  readonly sub?: string;
  readonly font?: "sans" | "mono";
  readonly color?: string;
}

export interface RowData {
  readonly cells: readonly CellData[];
}

export interface RelatedLink {
  readonly name: string;
  readonly route: string;
  readonly meta: string;
}

export interface BarItem {
  readonly label: string;
  readonly value: string;
  readonly pct: string;
  readonly color: string;
}

export interface SeriesItem {
  readonly label: string;
  readonly value: string;
  readonly h: string;
  readonly color: string;
}

export interface FlowStep {
  readonly label: string;
  readonly sub: string;
  readonly color: string;
}

export interface FlowChain {
  readonly name: string;
  readonly meta: string;
  readonly steps: readonly FlowStep[];
}

export type VizData =
  | { readonly kind: "bars"; readonly title: string; readonly hint: string; readonly items: readonly BarItem[] }
  | { readonly kind: "series"; readonly title: string; readonly hint: string; readonly items: readonly SeriesItem[] }
  | { readonly kind: "flow"; readonly title: string; readonly hint: string; readonly chains: readonly FlowChain[] }
  | { readonly kind: "split"; readonly title: string; readonly hint: string; readonly items: readonly BarItem[] };

export interface CopilotData {
  readonly text: string;
  readonly action: string;
}

export interface ScreenConfig {
  readonly title: string;
  readonly desc: string;
  readonly primary: string;
  readonly secondary: string;
  readonly kpis: readonly KpiItem[];
  readonly cols: readonly string[];
  readonly grid: string;
  readonly rows: readonly RowData[];
  readonly viz: VizData;
  readonly graphNote: string;
  readonly related: readonly RelatedLink[];
  readonly copilot: CopilotData;
  readonly rowActs: readonly string[];
}

const c = (t: string, sub?: string, color?: string): CellData =>
  sub ? { t, sub, font: "sans", color: color ?? "#1c1b18" } : { t, font: "sans", color: color ?? "#1c1b18" };
const m = (t: string, color?: string): CellData => ({ t, font: "mono", color: color ?? "#3d3a34" });
const rel = (name: string, route: string, meta: string): RelatedLink => ({ name, route, meta });

export const screenConfig = (key: string): ScreenConfig =>
  SCREEN_CONFIGS[key] ?? { title: key, desc: "", primary: "", secondary: "", kpis: [], cols: [], grid: "", rows: [], viz: { kind: "bars", title: "", hint: "", items: [] }, graphNote: "", related: [], copilot: { text: "", action: "" }, rowActs: [] };

export const SCREEN_CONFIGS: Record<string, ScreenConfig> = {
  reset: {
    title: "New session",
    desc: "Clear this period and start again — another month, another client, or another jurisdiction.",
    primary: "Reset and start over",
    secondary: "Archive first",
    kpis: [
      { label: "IN THIS SESSION", value: "848 cases", sub: "327 still undecided", color: "#a86a2c" },
      { label: "DECISIONS MADE", value: "521", sub: "kept in the audit log", color: "#2f6b4d" },
      { label: "EXPORTS TAKEN", value: "4", sub: "safe to reset", color: "#2f6b4d" },
      { label: "ARCHIVED PERIODS", value: "4", sub: "Apr – Jul, read-only", color: "#1c1b18" },
    ],
    grid: "1.3fr 1fr 1.4fr 130px",
    cols: ["WHAT RESETS", "WHAT SURVIVES", "WHY", "STATE"],
    rows: [
      { cells: [c("Uploaded files"), c("Import history"), c("You can re-import the same file version"), m("CLEARED", "#a86a2c")] },
      { cells: [c("Column mappings"), c("Saved as a template"), c("Next month reuses this mapping"), m("KEPT", "#2f6b4d")] },
      { cells: [c("Reconciliation results"), c("Closed periods"), c("Apr – Jul stay browsable and read-only"), m("KEPT", "#2f6b4d")] },
      { cells: [c("Open cases and drafts"), c("Decisions and approvals"), c("An audit trail you cannot delete by resetting"), m("LOGGED", "#41508f")] },
      { cells: [c("Selected period"), c("Client and GSTINs"), c("Switch month or jurisdiction from the header"), m("CLEARED", "#a86a2c")] },
    ],
    viz: {
      kind: "bars",
      title: "BEFORE YOU RESET",
      hint: "two of four checks pass",
      items: [
        { label: "Exports taken", value: "4 files", pct: "100%", color: "#2f6b4d" },
        { label: "Approvals cleared", value: "9 waiting", pct: "40%", color: "#a86a2c" },
        { label: "Cases decided", value: "521 of 848", pct: "61%", color: "#a86a2c" },
        { label: "Period filed", value: "3B not filed", pct: "4%", color: "#a13f28" },
      ],
    },
    graphNote: "Resetting clears the working session, not the graph. Facts, evidence and resolved supplier identities stay in GraphOWL, which is why next month starts warm rather than from nothing.",
    related: [rel("Deliverables", "deliverables", "export first"), rel("Periods", "periods", "archived months"), rel("Imports", "imports", "file history")],
    copilot: { text: "August is not filed and 9 approvals are still waiting. Resetting now would leave those decisions unmade against a period you have already reported on.", action: "Archive instead" },
    rowActs: ["Archive this period", "Export before reset", "Reset now"],
  },

  deliverables: {
    title: "Deliverables",
    desc: "What leaves the system at the end of a period — for the client, the CA and your own file.",
    primary: "Generate client report",
    secondary: "Working paper (.xlsx)",
    kpis: [
      { label: "READY TO EXPORT", value: "3 of 6", sub: "3 need a decision first", color: "#a86a2c" },
      { label: "GENERATED THIS PERIOD", value: "12", sub: "3 regenerated", color: "#1c1b18" },
      { label: "SENT TO CLIENT", value: "4", sub: "last on 15 Aug", color: "#2f6b4d" },
      { label: "RETAINED", value: "8 years", sub: "with the evidence behind them", color: "#1c1b18" },
    ],
    grid: "1.4fr 110px 1.3fr 140px 130px",
    cols: ["DELIVERABLE", "FORMAT", "WHAT IT CONTAINS", "STATE", "TAKE IT AWAY"],
    rows: [
      { cells: [c("GSTR-3B working paper", "Table 4 build-up"), m(".xlsx"), c("4A5, 4B2 and 4C with the invoices behind each"), m("READY", "#2f6b4d"), c("Download · Print")] },
      { cells: [c("ITC register", "invoice level"), m(".xlsx"), c("12,482 rows with reason code and citation"), m("READY", "#2f6b4d"), c("Download")] },
      { cells: [c("Reconciliation summary", "CA review copy"), m(".csv"), c("Every bucket, exception and decision"), m("READY", "#2f6b4d"), c("Download · Copy")] },
      { cells: [c("Client report", "plain language"), m(".md / PDF"), c("Findings, risk, and what you did about it"), m("DRAFTED", "#41508f"), c("Copy · .md · Print")] },
      { cells: [c("Supplier follow-up pack", "6 letters"), m(".md"), c("One letter per supplier, invoices cited"), m("NEEDS APPROVAL", "#a86a2c"), c("Review first")] },
      { cells: [c("IMS action sheet", "portal checklist"), m(".csv"), c("Accept, reject or pending per record"), m("NEEDS DECISION", "#a86a2c"), c("24 pending")] },
    ],
    viz: {
      kind: "bars",
      title: "EXPORT READINESS",
      hint: "what is blocking the rest",
      items: [
        { label: "Ready now", value: "3 of 6", pct: "50%", color: "#2f6b4d" },
        { label: "Drafted, needs a read", value: "1", pct: "17%", color: "#41508f" },
        { label: "Needs your approval", value: "1", pct: "17%", color: "#a86a2c" },
        { label: "Needs 24 IMS decisions", value: "1", pct: "17%", color: "#a13f28" },
      ],
    },
    graphNote: "Every export carries the case ids and fact ids behind each number, so a reviewer can walk from a figure in the working paper back to the source row it came from without asking you.",
    related: [rel("Approvals", "approvals", "sign-off first"), rel("IMS", "ims", "24 pending"), rel("New session", "reset", "when it is all out")],
    copilot: { text: "The client report is drafted but reads as if the 24 IMS decisions are already made. Worth deciding those before it goes out.", action: "Open the IMS queue" },
    rowActs: ["Download", "Copy to clipboard", "Print / PDF"],
  },

  authority: {
    title: "Authority",
    desc: "The law behind every flag. A finding you cannot cite is a finding you cannot defend to a client or an officer.",
    primary: "Add to working paper",
    secondary: "Rule reference",
    kpis: [
      { label: "FLAGS WITH A CITATION", value: "810 of 848", sub: "38 are firm policy", color: "#2f6b4d" },
      { label: "SECTIONS RELIED ON", value: "11", sub: "CGST Act, rules, advisories", color: "#1c1b18" },
      { label: "AMENDED THIS YEAR", value: "3", sub: "IMS rules, Oct 2025", color: "#a86a2c" },
      { label: "UNCITED IN A REPORT", value: "0", sub: "blocked from export", color: "#2f6b4d" },
    ],
    grid: "1.3fr 1fr 1.3fr 96px 110px",
    cols: ["WHAT WE FLAG", "AUTHORITY", "WHAT IT REQUIRES", "CASES", "STATUS"],
    rows: [
      { cells: [c("Supplier has not filed", "ITC not available yet"), m("s.16(2)(aa) CGST"), c("Credit only if the supplier reported the invoice"), m("42"), m("IN FORCE", "#2f6b4d")] },
      { cells: [c("Claim window closing", "₹8.2 L at stake"), m("s.16(4) CGST"), c("By 30 Nov of the next year, or the annual return"), m("92"), m("IN FORCE", "#2f6b4d")] },
      { cells: [c("Goods received after invoice"), m("s.16(2)(b) CGST"), c("No credit before the goods actually arrive"), m("14"), m("IN FORCE", "#2f6b4d")] },
      { cells: [c("Possible blocked credit"), m("s.17(5) CGST"), c("Named categories can never be claimed"), m("11"), m("IN FORCE", "#a13f28")] },
      { cells: [c("IMS inaction is acceptance"), m("Rule 60 + IMS advisory"), c("No action means the record is deemed accepted"), m("24"), m("AMENDED 2025", "#a86a2c")] },
      { cells: [c("Credit note held too long"), m("IMS advisory Oct 2025"), c("A credit note may be kept pending one period only"), m("3"), m("AMENDED 2025", "#a86a2c")] },
      { cells: [c("Rounding tolerance ± ₹10", "not a legal test"), m("— firm policy"), c("Differences under ₹10 are not chased"), m("96"), m("POLICY", "#6f6b62")] },
    ],
    viz: {
      kind: "bars",
      title: "EXPOSURE BY STATUTORY BASIS",
      hint: "which section each rupee at risk rests on",
      items: [
        { label: "s.16(2)(aa) · supplier filed", value: "42 · ₹4.2 L", pct: "100%", color: "#a13f28" },
        { label: "s.16(4) · time limit", value: "92 · ₹8.2 L", pct: "88%", color: "#a86a2c" },
        { label: "s.16(2)(b) · goods received", value: "14 · ₹1.6 L", pct: "33%", color: "#a86a2c" },
        { label: "s.17(5) · blocked credit", value: "11 · ₹1.4 L", pct: "26%", color: "#a13f28" },
        { label: "firm policy · tolerance", value: "96 · ₹0.6 L", pct: "14%", color: "#c3bdb2" },
      ],
    },
    graphNote: "Citations arrive with the GST knowledge pack, so when the law changes the pack version changes and every affected case is re-explained under the new authority. Nothing is hard-coded into the product.",
    related: [rel("Obligations", "obligations", "dated duties"), rel("Eligibility", "eligibility", "38 flagged"), rel("Rules", "rules", "how flags are set")],
    copilot: { text: "38 flags rest on firm policy rather than law — all of them the ₹10 rounding tolerance. Worth saying so explicitly in the client report.", action: "Label the 38 as policy" },
    rowActs: ["Open the section", "View flagged cases", "Cite in working paper"],
  },

  obligations: {
    title: "Obligations",
    desc: "What is due, when, and what lapses if you miss it.",
    primary: "Add to calendar",
    secondary: "Notify owners",
    kpis: [
      { label: "DUE IN 30 DAYS", value: "4", sub: "2 need action now", color: "#a86a2c" },
      { label: "AT RISK OF LAPSING", value: "₹8.2 L", sub: "92 invoices", color: "#a13f28" },
      { label: "RECURRING", value: "6", sub: "monthly and annual", color: "#1c1b18" },
      { label: "MISSED EVER", value: "0", sub: "across 12 periods", color: "#2f6b4d" },
    ],
    grid: "1.4fr 110px 1.2fr 110px 120px",
    cols: ["OBLIGATION", "DUE", "WHY IT MATTERS", "EXPOSURE", "STATE"],
    rows: [
      { cells: [c("GSTR-3B · August 2026", "monthly return"), m("20 Sep"), c("IMS records left untouched are accepted on filing"), m("₹12.4 L", "#a13f28"), m("OPEN", "#a86a2c")] },
      { cells: [c("IMS actions before 2B recompute", "draft 2B on the 14th"), m("14 Sep"), c("Actions after this force a recompute"), m("24 rec"), m("DUE SOON", "#a86a2c")] },
      { cells: [c("s.16(4) claim window · FY 2025-26"), m("30 Nov"), c("Unclaimed credit lapses permanently"), m("₹8.2 L", "#a13f28"), m("AT RISK", "#a13f28")] },
      { cells: [c("Credit note held in IMS", "one period only"), m("30 Sep"), c("Must be accepted or rejected next period"), m("3 rec"), m("OPEN", "#a86a2c")] },
      { cells: [c("GSTR-1 · September 2026", "outward supplies"), m("11 Oct"), c("Your own filing — it feeds your buyers' 2B"), m("—"), m("SCHEDULED", "#6f6b62")] },
      { cells: [c("Annual return · FY 2025-26", "GSTR-9"), m("31 Dec"), c("Closes the year and the claim window with it"), m("—"), m("SCHEDULED", "#6f6b62")] },
    ],
    viz: {
      kind: "series",
      title: "DAYS LEFT ON EACH DEADLINE",
      hint: "shorter bar means less time",
      items: [
        { label: "2B", value: "28 d", h: "28%", color: "#e0b98f" },
        { label: "3B", value: "34 d", h: "34%", color: "#e0b98f" },
        { label: "CN", value: "44 d", h: "44%", color: "#c6cee9" },
        { label: "G1", value: "55 d", h: "55%", color: "#c6cee9" },
        { label: "16(4)", value: "105 d", h: "100%", color: "#a13f28" },
        { label: "G9", value: "136 d", h: "100%", color: "#dcd7cc" },
      ],
    },
    graphNote: "Filing periods and claim windows are modelled as dated entities in the graph, which is what lets a case say \u201cclaimable until 30 November\u201d instead of just \u201cunmatched\u201d.",
    related: [rel("Authority", "authority", "the law behind each"), rel("Periods", "periods", "close lifecycle"), rel("IMS", "ims", "24 pending")],
    copilot: { text: "₹8.2 L sits inside the s.16(4) window with 105 days left. Last year the same pool shrank to ₹1.1 L once follow-ups went out in September.", action: "Start the September chase" },
    rowActs: ["Open the work", "Assign owner", "Snooze reminder"],
  },

  periods: {
    title: "Periods",
    desc: "Each filing month has its own lifecycle. Nothing closes with silent exceptions.",
    primary: "Close August",
    secondary: "Period comparison",
    kpis: [
      { label: "CURRENT", value: "Aug 2026", sub: "open · 38 exceptions", color: "#1c1b18" },
      { label: "CLOSED FY-TD", value: "4", sub: "Apr – Jul", color: "#1c1b18" },
      { label: "CARRIED FORWARD", value: "47", sub: "cross-period invoices", color: "#6b4fa8" },
      { label: "AVG CLOSE TIME", value: "6.2 d", sub: "from 2B generation", color: "#1c1b18" },
    ],
    grid: "104px 110px 110px 110px 110px 1fr",
    cols: ["PERIOD", "BOOKS", "GSTR-2B", "IMS", "3B", "STATE"],
    rows: [
      { cells: [c("August 2026", "current"), m("Imported", "#2f6b4d"), m("Generated", "#2f6b4d"), m("24 pending", "#a86a2c"), m("Not filed", "#6f6b62"), c("Open · 38 exceptions, ₹4.2L exposure")] },
      { cells: [c("July 2026"), m("Imported", "#2f6b4d"), m("Generated", "#2f6b4d"), m("Complete", "#2f6b4d"), m("Filed 20 Aug", "#2f6b4d"), c("Closed with 9 exceptions")] },
      { cells: [c("June 2026"), m("Imported", "#2f6b4d"), m("Generated", "#2f6b4d"), m("Complete", "#2f6b4d"), m("Filed 20 Jul", "#2f6b4d"), c("Closed clean")] },
      { cells: [c("May 2026"), m("Imported", "#2f6b4d"), m("Generated", "#2f6b4d"), m("Complete", "#2f6b4d"), m("Filed 20 Jun", "#2f6b4d"), c("Closed with 14 exceptions")] },
      { cells: [c("April 2026"), m("Imported", "#2f6b4d"), m("Generated", "#2f6b4d"), m("Complete", "#2f6b4d"), m("Filed 20 May", "#2f6b4d"), c("Closed clean")] },
    ],
    viz: {
      kind: "flow",
      title: "PERIOD LIFECYCLE · AUGUST 2026",
      hint: "the close gate will not pass with unresolved exposure",
      chains: [
        {
          name: "AUGUST",
          meta: "day 17 of the cycle",
          steps: [
            { label: "Books imported", sub: "15 Aug", color: "#2f6b4d" },
            { label: "2B generated", sub: "14 Aug", color: "#2f6b4d" },
            { label: "Reconciled", sub: "848 exceptions", color: "#2f6b4d" },
            { label: "IMS actions", sub: "24 pending", color: "#a86a2c" },
            { label: "Close period", sub: "38 open", color: "#a13f28" },
            { label: "File 3B", sub: "due 20 Sep", color: "#c3bdb2" },
          ],
        },
      ],
    },
    graphNote: "The graph models the filing period as its own entity, so an invoice can belong to July in your books and August in 2B without either being wrong.",
    related: [rel("Cross-period", "crossperiod", "47 invoices"), rel("Exceptions", "exceptions", "38 open"), rel("ITC position", "itc", "₹3.42 Cr")],
    copilot: { text: "Two blockers stand between you and closing August: 24 IMS records with no action, and ₹4.2 L of exposure on 11 suppliers with no follow-up sent.", action: "Draft the 11 follow-ups" },
    rowActs: ["Open period", "Close with exceptions", "Export working"],
  },

  exceptions: {
    title: "Exceptions by reason",
    desc: "Reason groups for the whole period. Pick a group to get its invoices; open one invoice to get its case detail.",
    primary: "Assign selected",
    secondary: "Bulk export",
    kpis: [
      { label: "OPEN", value: "848", sub: "327 need a decision", color: "#a13f28" },
      { label: "RESOLVED THIS MONTH", value: "1,204", sub: "62% within 3 days", color: "#2f6b4d" },
      { label: "UNASSIGNED", value: "96", sub: "no owner", color: "#a86a2c" },
      { label: "OLDEST", value: "38 d", sub: "supplier not filed", color: "#a13f28" },
    ],
    grid: "1.5fr 90px 116px 116px 130px",
    cols: ["REASON", "COUNT", "ITC EXPOSURE", "OWNER", "SLA"],
    rows: [
      { cells: [c("Supplier has not filed GSTR-1", "chase the supplier"), m("42"), m("₹4.2L", "#a13f28"), c("Sneha"), m("12 breached", "#a13f28")] },
      { cells: [c("Tax amount differs", "verify GSTR-1 value"), m("181"), m("₹3.1L", "#a13f28"), c("Sneha"), m("on track", "#2f6b4d")] },
      { cells: [c("Only in books", "not reported by supplier"), m("263"), m("₹1.8L", "#a86a2c"), c("Rahul"), m("4 at risk", "#a86a2c")] },
      { cells: [c("Only in 2B", "unrecorded purchase"), m("125"), m("₹1.4L", "#a86a2c"), c("Unassigned", "#a86a2c"), m("—")] },
      { cells: [c("Cross-period", "late supplier filing"), m("47"), m("₹1.4L"), c("Auto-resolved"), m("n/a")] },
      { cells: [c("Possible duplicate", "same GSTIN, amount, date"), m("18"), m("₹0.8L", "#a86a2c"), c("Rahul"), m("on track", "#2f6b4d")] },
      { cells: [c("GSTIN transposition", "suggested fix available"), m("9"), m("₹0.4L"), c("Auto-suggested"), m("n/a")] },
    ],
    viz: {
      kind: "bars",
      title: "EXPOSURE BY REASON",
      hint: "width = ITC at risk, not case count",
      items: [
        { label: "Supplier not filed", value: "₹4.2 L · 42", pct: "100%", color: "#a13f28" },
        { label: "Tax amount differs", value: "₹3.1 L · 181", pct: "74%", color: "#a86a2c" },
        { label: "Only in books", value: "₹1.8 L · 263", pct: "43%", color: "#a86a2c" },
        { label: "Only in 2B", value: "₹1.4 L · 125", pct: "33%", color: "#5b6bb5" },
        { label: "Cross-period", value: "₹1.4 L · 47", pct: "33%", color: "#6b4fa8" },
        { label: "Duplicate", value: "₹0.8 L · 18", pct: "19%", color: "#a29d93" },
      ],
    },
    graphNote: "Reason codes are not string rules. Each one is a query over the evidence graph, which is why a case can carry its own explanation and citations.",
    related: [rel("Register", "register", "invoice level"), rel("Follow-ups", "followups", "12 open"), rel("Review queue", "queue", "work sequentially")],
    copilot: { text: "96 of the 181 amount mismatches are under ₹1,000 and sit inside your rounding tolerance pattern from July.", action: "Group them for bulk accept" },
    rowActs: ["Open cases", "Assign owner", "Create follow-ups"],
  },

  crossperiod: {
    title: "Cross-period",
    desc: "Invoice date, books period and 2B period are three different things.",
    primary: "Accept all suggested",
    secondary: "Explain rule",
    kpis: [
      { label: "CROSS-PERIOD", value: "47", sub: "this month", color: "#6b4fa8" },
      { label: "AUTO-MATCHED", value: "38", sub: "by the graph", color: "#2f6b4d" },
      { label: "NEEDS REVIEW", value: "9", sub: "ambiguous", color: "#a86a2c" },
      { label: "ITC DEFERRED", value: "₹1.4L", sub: "claimable in August", color: "#1c1b18" },
    ],
    grid: "110px 1fr 104px 104px 104px 110px",
    cols: ["INVOICE", "SUPPLIER", "INV DATE", "BOOKS", "2B PERIOD", "RESULT"],
    rows: [
      { cells: [m("INV-0987"), c("Kalyan Polymers"), m("28 Jul"), m("July"), m("August"), m("MATCHED", "#2f6b4d")] },
      { cells: [m("INV-0991"), c("Trident Chemicals"), m("30 Jul"), m("July"), m("August"), m("MATCHED", "#2f6b4d")] },
      { cells: [m("INV-0994"), c("Meridian Steel Co"), m("31 Jul"), m("July"), m("not yet"), m("PENDING", "#a86a2c")] },
      { cells: [m("INV-1002"), c("Sarvottam Traders"), m("02 Aug"), m("August"), m("September?"), m("WATCH", "#a86a2c")] },
      { cells: [m("INV-0975"), c("XYZ Pvt Ltd"), m("22 Jul"), m("July"), m("August"), m("MATCHED", "#2f6b4d")] },
    ],
    viz: {
      kind: "flow",
      title: "WHY AN INVOICE LANDS IN A LATER 2B",
      hint: "the graph walks this path for every cross-period case",
      chains: [
        {
          name: "INV-0987",
          meta: "auto-matched, 0.98",
          steps: [
            { label: "Invoice 28 Jul", sub: "books · July", color: "#c3bdb2" },
            { label: "Supplier files", sub: "GSTR-1 on 11 Aug", color: "#a86a2c" },
            { label: "August period", sub: "gst:FilingPeriod", color: "#6b4fa8" },
            { label: "GSTR-2B Aug", sub: "ITC claimable now", color: "#2f6b4d" },
          ],
        },
      ],
    },
    graphNote: "Invoice → supplier filing → FilingPeriod is a two-hop path in the graph. That path is what lets the system say 'late filing', not a guess based on dates.",
    related: [rel("Periods", "periods", "lifecycle"), rel("Exception case", "case", "see one"), rel("Suppliers", "suppliers", "who files late")],
    copilot: { text: "38 of 47 cross-period cases resolved on the filing-date path alone. The remaining 9 have no supplier filing in either period.", action: "Escalate the 9 to follow-up" },
    rowActs: ["Accept match", "Move to September", "Inspect evidence"],
  },

  itc: {
    title: "ITC position",
    desc: "August 2026. Language here is deliberately cautious — the system does not certify eligibility.",
    primary: "Export working",
    secondary: "Reason breakdown",
    kpis: [
      { label: "BOOKS ITC", value: "₹3.42 Cr", sub: "12,482 invoices", color: "#1c1b18" },
      { label: "AVAILABLE IN 2B", value: "₹3.18 Cr", sub: "12,197 invoices", color: "#1c1b18" },
      { label: "RECONCILED", value: "₹2.91 Cr", sub: "matched on all fields", color: "#2f6b4d" },
      { label: "AT RISK", value: "₹12.4 L", sub: "146 invoices", color: "#a13f28" },
    ],
    grid: "1.5fr 130px 110px 1fr",
    cols: ["CATEGORY", "AMOUNT", "INVOICES", "WHAT IT MEANS"],
    rows: [
      { cells: [c("Reconciled, eligible per available data"), m("₹2.91 Cr", "#2f6b4d"), m("9,842"), c("All identifiers and tax heads agree with 2B")] },
      { cells: [c("Needs review"), m("₹18.7 L", "#a86a2c"), m("327"), c("Present in both, something differs")] },
      { cells: [c("Not available in 2B"), m("₹12.4 L", "#a13f28"), m("146"), c("Supplier has not reported it yet")] },
      { cells: [c("Potentially ineligible"), m("₹4.2 L", "#a13f28"), m("38"), c("Blocked credit, RCM or goods not received")] },
      { cells: [c("Recoverable next period"), m("₹8.2 L"), m("92"), c("Expected in September 2B on amendment")] },
    ],
    viz: {
      kind: "split",
      title: "ITC POSITION · ₹3.42 Cr IN BOOKS",
      hint: "one bar, every rupee accounted for",
      items: [
        { label: "Reconciled", value: "₹2.91 Cr", pct: "85%", color: "#2f6b4d" },
        { label: "Needs review", value: "₹18.7 L", pct: "5.5%", color: "#a86a2c" },
        { label: "At risk", value: "₹12.4 L", pct: "3.6%", color: "#a13f28" },
        { label: "Potentially ineligible", value: "₹4.2 L", pct: "1.2%", color: "#6b4fa8" },
        { label: "Recoverable later", value: "₹8.2 L", pct: "2.4%", color: "#5b6bb5" },
      ],
    },
    graphNote: "\u201cEligible per available data\u201d is as far as the system will go. Self-assessment cases the 2B classification does not capture stay flagged for you.",
    related: [rel("At risk", "atrisk", "₹12.4L"), rel("Eligibility", "eligibility", "38 flagged"), rel("Exceptions", "exceptions", "848")],
    copilot: { text: "₹8.2 L of the at-risk pool matches the profile that got recovered last period — suppliers who filed one cycle late.", action: "Build a recovery watchlist" },
    rowActs: ["Open invoices", "Export schedule", "Flag for review"],
  },

  atrisk: {
    title: "ITC at risk",
    desc: "Ranked by money, then by how long it has been sitting there.",
    primary: "Create follow-ups",
    secondary: "Notify owners",
    kpis: [
      { label: "TOTAL AT RISK", value: "₹12.4 L", sub: "146 invoices", color: "#a13f28" },
      { label: "SUPPLIERS", value: "42", sub: "to be contacted", color: "#1c1b18" },
      { label: "OVER 30 DAYS", value: "₹3.6 L", sub: "24 invoices", color: "#a13f28" },
      { label: "RECOVERED LAST MONTH", value: "₹5.1 L", sub: "after follow-up", color: "#2f6b4d" },
    ],
    grid: "1.4fr 116px 96px 130px 120px",
    cols: ["SUPPLIER", "EXPOSURE", "INVOICES", "REASON", "AGE"],
    rows: [
      { cells: [c("XYZ Pvt Ltd", "29XXXXX4321X1Z9"), m("₹4.82 L", "#a13f28"), m("18"), c("Amount + late filing"), m("38 d", "#a13f28")] },
      { cells: [c("ABC Suppliers", "27XXXXX7788X1Z3"), m("₹3.17 L", "#a13f28"), m("12"), c("Not in 2B"), m("26 d", "#a13f28")] },
      { cells: [c("PQR Industries", "27XXXXX8899X1Z2"), m("₹2.41 L", "#a86a2c"), m("9"), c("Not filed"), m("19 d", "#a86a2c")] },
      { cells: [c("Meridian Steel Co", "24XXXXX1122X1Z7"), m("₹0.96 L", "#a86a2c"), m("4"), c("Not filed"), m("12 d")] },
      { cells: [c("Northline Logistics", "07XXXXX3344X1Z6"), m("₹0.58 L"), m("3"), c("Only in 2B"), m("6 d")] },
    ],
    viz: {
      kind: "bars",
      title: "EXPOSURE BY SUPPLIER",
      hint: "chase from the top — 5 suppliers hold 74% of the risk",
      items: [
        { label: "XYZ Pvt Ltd", value: "₹4.82 L · 38 d", pct: "100%", color: "#a13f28" },
        { label: "ABC Suppliers", value: "₹3.17 L · 26 d", pct: "66%", color: "#a13f28" },
        { label: "PQR Industries", value: "₹2.41 L · 19 d", pct: "50%", color: "#a86a2c" },
        { label: "Meridian Steel", value: "₹0.96 L · 12 d", pct: "20%", color: "#a86a2c" },
        { label: "Northline Logistics", value: "₹0.58 L · 6 d", pct: "12%", color: "#5b6bb5" },
      ],
    },
    graphNote: "Supplier-level exposure aggregates across every invoice the graph resolved to that entity — including records filed under a slightly different legal name.",
    related: [rel("Follow-ups", "followups", "draft messages"), rel("Supplier risk", "risk", "behaviour"), rel("Assistants", "agents", "drafted 6")],
    copilot: { text: "XYZ, ABC and PQR account for 74% of exposure and have all been contacted before. Their reply rate is 60% within 9 days.", action: "Draft a firmer second notice" },
    rowActs: ["Draft follow-up", "Assign owner", "Mark chased"],
  },

  eligibility: {
    title: "Eligibility",
    desc: "Cases where the invoice matches but the credit may still not be claimable.",
    primary: "Mark decisions",
    secondary: "Rule reference",
    kpis: [
      { label: "FLAGGED", value: "38", sub: "₹4.2 L", color: "#a86a2c" },
      { label: "GOODS NOT RECEIVED", value: "14", sub: "timing issue", color: "#a86a2c" },
      { label: "BLOCKED CREDIT", value: "11", sub: "section 17(5) candidates", color: "#a13f28" },
      { label: "DECIDED", value: "13", sub: "by you, this month", color: "#2f6b4d" },
    ],
    grid: "110px 1fr 116px 130px 130px",
    cols: ["INVOICE", "SUPPLIER", "AMOUNT", "FLAG", "EVIDENCE"],
    rows: [
      { cells: [m("INV-1025"), c("XYZ Pvt Ltd"), m("₹38,000"), c("Goods received 18 Aug", "after invoice date"), c("GRN 4482")] },
      { cells: [m("INV-1088"), c("Sarvottam Traders"), m("₹64,200"), c("Possible blocked credit", "motor vehicle"), c("Item master")] },
      { cells: [m("INV-1094"), c("Trident Chemicals"), m("₹22,900"), c("RCM — self-invoice missing"), c("Books only")] },
      { cells: [m("INV-1101"), c("Anand Auto Components"), m("₹18,400"), c("Goods in transit at period end"), c("GRN pending")] },
      { cells: [m("INV-1112"), c("Northline Logistics"), m("₹12,600"), c("ISD credit — wrong GSTIN"), c("2B category")] },
    ],
    viz: {
      kind: "bars",
      title: "WHY CREDIT MAY NOT BE CLAIMABLE",
      hint: "matched in 2B, still not automatically eligible",
      items: [
        { label: "Goods not yet received", value: "14 · ₹1.6 L", pct: "100%", color: "#a86a2c" },
        { label: "Possible blocked credit", value: "11 · ₹1.4 L", pct: "79%", color: "#a13f28" },
        { label: "RCM self-invoice missing", value: "7 · ₹0.7 L", pct: "50%", color: "#a86a2c" },
        { label: "ISD / wrong GSTIN", value: "6 · ₹0.5 L", pct: "43%", color: "#5b6bb5" },
      ],
    },
    graphNote: "The graph models goods receipt as an event separate from the invoice, so a timing issue looks like a timing issue rather than a mismatch.",
    related: [rel("ITC position", "itc", "summary"), rel("Exception case", "case", "worked example"), rel("Rules", "rules", "eligibility rules")],
    copilot: { text: "14 cases are timing, not eligibility — goods arrive after the invoice. They become claimable next period without any action.", action: "Move 14 to September" },
    rowActs: ["Mark ineligible", "Claim anyway", "Ask preparer"],
  },

  suppliers: {
    title: "Suppliers",
    desc: "One row per resolved supplier entity, across books, GSTR-1 and 2B.",
    primary: "Contact selected",
    secondary: "Segment",
    kpis: [
      { label: "ACTIVE SUPPLIERS", value: "1,482", sub: "this period", color: "#1c1b18" },
      { label: "WITH EXCEPTIONS", value: "214", sub: "14%", color: "#a86a2c" },
      { label: "HIGH RISK", value: "28", sub: "behaviour-scored", color: "#a13f28" },
      { label: "MERGED IDENTITIES", value: "96", sub: "by entity resolution", color: "#41508f" },
    ],
    grid: "1.4fr 90px 90px 96px 116px 96px",
    cols: ["SUPPLIER", "INVOICES", "MATCHED", "MISMATCH", "ITC AT RISK", "RISK"],
    rows: [
      { cells: [c("XYZ Pvt Ltd", "29XXXXX4321X1Z9 · 2 identities merged"), m("382"), m("344"), m("18", "#a86a2c"), m("₹4.82 L", "#a13f28"), m("HIGH", "#a13f28")] },
      { cells: [c("ABC Suppliers", "27XXXXX7788X1Z3"), m("294"), m("271"), m("11", "#a86a2c"), m("₹3.17 L", "#a13f28"), m("HIGH", "#a13f28")] },
      { cells: [c("PQR Industries", "27XXXXX8899X1Z2"), m("188"), m("176"), m("6"), m("₹2.41 L", "#a86a2c"), m("MEDIUM", "#a86a2c")] },
      { cells: [c("Kalyan Polymers", "27XXXXX5566X1Z4"), m("142"), m("141"), m("1"), m("₹0.12 L"), m("LOW", "#2f6b4d")] },
      { cells: [c("Trident Chemicals", "33XXXXX7788X1Z1"), m("118"), m("112"), m("4"), m("₹0.61 L"), m("MEDIUM", "#a86a2c")] },
    ],
    viz: {
      kind: "bars",
      title: "PORTFOLIO BY EXCEPTION RATE",
      hint: "214 of 1,482 suppliers produced an exception this period",
      items: [
        { label: "Clean", value: "1,268", pct: "86%", color: "#2f6b4d" },
        { label: "1 – 5 exceptions", value: "158", pct: "11%", color: "#a86a2c" },
        { label: "6 – 20 exceptions", value: "46", pct: "3%", color: "#a13f28" },
        { label: "Over 20", value: "10", pct: "1%", color: "#a13f28" },
      ],
    },
    graphNote: "\u201c2 identities merged\u201d means the graph resolved two source records — an ERP master row and a CRM account — to one supplier at 0.97 confidence. You can inspect and reverse that.",
    related: [rel("Supplier risk", "risk", "behaviour detail"), rel("At risk", "atrisk", "exposure"), rel("Follow-ups", "followups", "chase list")],
    copilot: { text: "96 suppliers were merged from two source records this period. Two of those merges look weak — similar names, no shared PAN.", action: "Review the 2 weak merges" },
    rowActs: ["Open supplier", "Draft follow-up", "View graph"],
  },

  risk: {
    title: "Supplier risk",
    desc: "Behaviour observed over 12 periods, traceable to individual filings.",
    primary: "Export scorecard",
    secondary: "Adjust weights",
    kpis: [
      { label: "SCORED", value: "1,482", sub: "12-period window", color: "#1c1b18" },
      { label: "HIGH RISK", value: "28", sub: "₹9.4 L exposure", color: "#a13f28" },
      { label: "IMPROVED", value: "44", sub: "vs last quarter", color: "#2f6b4d" },
      { label: "NEW TO PORTFOLIO", value: "19", sub: "unscored", color: "#6f6b62" },
    ],
    grid: "1.3fr 116px 116px 116px 116px",
    cols: ["SUPPLIER", "LATE FILING", "AMOUNT DIFFS", "CORRECTIONS", "GSTIN ERRORS"],
    rows: [
      { cells: [c("XYZ Pvt Ltd", "8 late filings, avg 18 d"), m("31% · high", "#a13f28"), m("7%", "#a86a2c"), m("3", "#a86a2c"), m("low", "#2f6b4d")] },
      { cells: [c("ABC Suppliers", "consistent 2B gaps"), m("24% · high", "#a13f28"), m("4%"), m("1"), m("low", "#2f6b4d")] },
      { cells: [c("PQR Industries", "improving"), m("12% · medium", "#a86a2c"), m("2%"), m("0"), m("medium", "#a86a2c")] },
      { cells: [c("Sarvottam Traders", "2 transpositions caught"), m("6% · low", "#2f6b4d"), m("1%"), m("0"), m("high", "#a13f28")] },
      { cells: [c("Kalyan Polymers", "clean"), m("2% · low", "#2f6b4d"), m("0%"), m("0"), m("low", "#2f6b4d")] },
    ],
    viz: {
      kind: "series",
      title: "LATE FILING RATE · XYZ PVT LTD",
      hint: "12 periods, each bar traceable to a filing date in the graph",
      items: [
        { label: "Sep", value: "8%", h: "16%", color: "#dcd7cc" },
        { label: "Oct", value: "14%", h: "28%", color: "#dcd7cc" },
        { label: "Nov", value: "22%", h: "44%", color: "#e0b98f" },
        { label: "Dec", value: "18%", h: "36%", color: "#e0b98f" },
        { label: "Jan", value: "26%", h: "52%", color: "#e0b98f" },
        { label: "Feb", value: "31%", h: "62%", color: "#c9803a" },
        { label: "Mar", value: "29%", h: "58%", color: "#c9803a" },
        { label: "Apr", value: "34%", h: "68%", color: "#c9803a" },
        { label: "May", value: "38%", h: "76%", color: "#a13f28" },
        { label: "Jun", value: "41%", h: "82%", color: "#a13f28" },
        { label: "Jul", value: "36%", h: "72%", color: "#a13f28" },
        { label: "Aug", value: "31%", h: "62%", color: "#a13f28" },
      ],
    },
    graphNote: "Each score decomposes into the facts behind it — eight late filings, three amount mismatches, two corrected invoices — so a risk label can always be defended to the supplier.",
    related: [rel("Suppliers", "suppliers", "portfolio"), rel("Exceptions", "exceptions", "their cases"), rel("Analytics", "analytics", "trends")],
    copilot: { text: "XYZ has slipped for four consecutive periods. Every score component traces to filing dates the graph holds.", action: "Export the evidence pack" },
    rowActs: ["Open supplier", "Export scorecard", "Flag high risk"],
  },

  followups: {
    title: "Follow-ups",
    desc: "Who needs chasing, why, and what has already been sent.",
    primary: "Send approved",
    secondary: "Template",
    kpis: [
      { label: "OPEN", value: "12", sub: "₹9.8 L exposure", color: "#a13f28" },
      { label: "AWAITING REPLY", value: "7", sub: "avg 9 days", color: "#a86a2c" },
      { label: "RESOLVED 30D", value: "34", sub: "₹5.1 L recovered", color: "#2f6b4d" },
      { label: "DRAFTED", value: "6", sub: "need your approval", color: "#41508f" },
    ],
    grid: "1.3fr 1fr 116px 120px 116px",
    cols: ["SUPPLIER", "ISSUE", "AMOUNT", "STATUS", "LAST CONTACT"],
    rows: [
      { cells: [c("XYZ Pvt Ltd", "5 invoices"), c("IGST difference"), m("₹4.1 L", "#a13f28"), m("WAITING", "#a86a2c"), m("09 Aug")] },
      { cells: [c("ABC Suppliers", "12 invoices"), c("Not reported in 2B"), m("₹3.17 L", "#a13f28"), m("SENT", "#41508f"), m("12 Aug")] },
      { cells: [c("PQR Industries", "9 invoices"), c("Late filing"), m("₹2.41 L", "#a86a2c"), m("OPEN", "#a13f28"), m("—")] },
      { cells: [c("Meridian Steel Co", "4 invoices"), c("GSTR-1 not filed"), m("₹0.96 L"), m("DRAFTED", "#41508f"), m("—")] },
      { cells: [c("Anand Auto Components", "1 invoice"), c("GSTIN transposition"), m("₹0.39 L"), m("RESOLVED", "#2f6b4d"), m("06 Aug")] },
    ],
    viz: {
      kind: "flow",
      title: "FOLLOW-UP LIFECYCLE",
      hint: "drafted by the assistant, sent by you",
      chains: [
        {
          name: "XYZ PVT LTD",
          meta: "₹4.1 L · 5 invoices",
          steps: [
            { label: "Case raised", sub: "11 Aug", color: "#c3bdb2" },
            { label: "Draft written", sub: "cites 5 invoices", color: "#5b6bb5" },
            { label: "You approved", sub: "12 Aug", color: "#2f6b4d" },
            { label: "Awaiting reply", sub: "9 days", color: "#a86a2c" },
            { label: "Amendment in 2B", sub: "expected Sep", color: "#c3bdb2" },
          ],
        },
      ],
    },
    graphNote: "A drafted message lists only invoices the graph can evidence — invoice number, declared value and the filing it appeared in — so the supplier can verify without a phone call.",
    related: [rel("Assistants", "agents", "6 drafts"), rel("At risk", "atrisk", "ranked"), rel("Suppliers", "suppliers", "portfolio")],
    copilot: { text: "7 threads have had no reply for over 9 days. Historically a second notice recovers 40% of them.", action: "Draft second notices" },
    rowActs: ["Open thread", "Resend", "Mark resolved"],
  },

  queue: {
    title: "Review queue",
    desc: "Work cases one at a time, grouped by reason. Keyboard-driven.",
    primary: "Start reviewing",
    secondary: "Assign to me",
    kpis: [
      { label: "IN QUEUE", value: "327", sub: "need a decision", color: "#1c1b18" },
      { label: "YOURS", value: "88", sub: "Sneha", color: "#1c1b18" },
      { label: "AVG TIME", value: "42 s", sub: "per case", color: "#2f6b4d" },
      { label: "DONE TODAY", value: "64", sub: "₹2.1 L cleared", color: "#2f6b4d" },
    ],
    grid: "1.5fr 96px 116px 120px 120px",
    cols: ["GROUP", "CASES", "EXPOSURE", "ASSIGNED", "SUGGESTED ACTION"],
    rows: [
      { cells: [c("Amount mismatch under ₹1,000", "likely rounding"), m("96"), m("₹0.6 L"), c("Sneha"), c("Bulk accept 2B value")] },
      { cells: [c("Amount mismatch over ₹1,000"), m("85"), m("₹2.5 L", "#a86a2c"), c("Sneha"), c("Follow up individually")] },
      { cells: [c("Missing in 2B — supplier not filed"), m("42"), m("₹4.2 L", "#a13f28"), c("Rahul"), c("Create follow-ups")] },
      { cells: [c("Only in 2B — book the purchase"), m("39"), m("₹1.4 L"), c("Unassigned", "#a86a2c"), c("Send to accounts")] },
      { cells: [c("GSTIN transposition suggested"), m("9"), m("₹0.4 L"), c("Auto"), c("Accept suggestion")] },
    ],
    viz: {
      kind: "bars",
      title: "QUEUE BURN-DOWN",
      hint: "327 cases open, 64 cleared today",
      items: [
        { label: "Cleared today", value: "64", pct: "20%", color: "#2f6b4d" },
        { label: "In progress", value: "88", pct: "27%", color: "#5b6bb5" },
        { label: "Untouched", value: "175", pct: "53%", color: "#a86a2c" },
      ],
    },
    graphNote: "Groups are built from the same reason queries the graph uses to raise the case, so a bulk decision applies to exactly the cases you inspected.",
    related: [rel("Exceptions", "exceptions", "all reasons"), rel("Register", "register", "invoice level"), rel("Approvals", "approvals", "sign-off")],
    copilot: { text: "Sorted by exposure, the top 40 cases carry 61% of the money in this queue. The rest are under ₹2,000 each.", action: "Queue the top 40 first" },
    rowActs: ["Start reviewing", "Reassign", "Bulk accept"],
  },

  ims: {
    title: "IMS",
    desc: "Accept, reject or keep pending. These actions change GSTR-2B on recompute.",
    primary: "Submit actions",
    secondary: "Sync IMS",
    kpis: [
      { label: "PENDING ACTION", value: "24", sub: "before 3B filing", color: "#a86a2c" },
      { label: "ACCEPTED", value: "11,904", sub: "this period", color: "#2f6b4d" },
      { label: "REJECTED", value: "38", sub: "₹1.2 L", color: "#a13f28" },
      { label: "DEEMED ACCEPTED", value: "6", sub: "if no action taken", color: "#6b4fa8" },
    ],
    grid: "104px 1.2fr 110px 110px 130px 120px",
    cols: ["INVOICE", "SUPPLIER", "BOOKS", "IMS", "CATEGORY", "ACTION"],
    rows: [
      { cells: [m("INV-1148"), c("XYZ Pvt Ltd"), m("₹47,800"), m("₹47,800"), c("B2B"), m("NO ACTION", "#a86a2c")] },
      { cells: [m("INV-1025"), c("XYZ Pvt Ltd"), m("₹38,000"), m("₹37,500", "#a13f28"), c("B2B"), m("REVIEW", "#a86a2c")] },
      { cells: [m("CN-0042"), c("Trident Chemicals"), m("−₹8,400"), m("−₹8,400"), c("Credit note"), m("ACCEPT", "#2f6b4d")] },
      { cells: [m("INV-1162"), c("Sarvottam Traders"), m("₹41,200"), m("₹41,200"), c("B2B · duplicate?"), m("PENDING", "#6b4fa8")] },
      { cells: [m("IMP-0018"), c("Global Imports SEZ"), m("₹1,12,000"), m("₹1,12,000"), c("Import of goods"), m("ACCEPT", "#2f6b4d")] },
    ],
    viz: {
      kind: "flow",
      title: "IMS → GSTR-2B → 3B",
      hint: "an IMS action recomputes 2B, so it is not just a status",
      chains: [
        {
          name: "CURRENT PERIOD",
          meta: "24 records pending",
          steps: [
            { label: "Supplier files", sub: "GSTR-1", color: "#c3bdb2" },
            { label: "IMS record", sub: "appears", color: "#6b4fa8" },
            { label: "Your action", sub: "accept / reject / pending", color: "#a86a2c" },
            { label: "2B recomputed", sub: "on change", color: "#5b6bb5" },
            { label: "GSTR-3B", sub: "deemed accepted if untouched", color: "#a13f28" },
          ],
        },
      ],
    },
    graphNote: "IMS is a separate evidence source in the graph, not a status column. That is why a record can be matched in reconciliation and still be awaiting an IMS decision.",
    related: [rel("Exception case", "case", "worked example"), rel("Periods", "periods", "before 3B"), rel("Review queue", "queue", "work through")],
    copilot: { text: "6 records will be deemed accepted when 3B is filed. Two of them are the disputed amounts from XYZ.", action: "Hold the 2 disputed as pending" },
    rowActs: ["Accept", "Reject", "Keep pending"],
  },

  approvals: {
    title: "Approvals",
    desc: "Decisions that need a second signature before the period closes.",
    primary: "Approve selected",
    secondary: "Delegate",
    kpis: [
      { label: "AWAITING", value: "9", sub: "₹6.4 L", color: "#a86a2c" },
      { label: "APPROVED 30D", value: "112", sub: "avg 1.2 d", color: "#2f6b4d" },
      { label: "RETURNED", value: "4", sub: "sent back for detail", color: "#a13f28" },
      { label: "THRESHOLD", value: "₹50,000", sub: "above this needs sign-off", color: "#1c1b18" },
    ],
    grid: "1.4fr 116px 120px 120px 130px",
    cols: ["DECISION", "AMOUNT", "REQUESTED BY", "TYPE", "WAITING"],
    rows: [
      { cells: [c("Write off ITC — supplier untraceable", "ABC Suppliers, 12 invoices"), m("₹3.17 L", "#a13f28"), c("Sneha"), c("Write-off"), m("3 d", "#a86a2c")] },
      { cells: [c("Accept 2B value over books", "96 rounding cases"), m("₹0.6 L"), c("Sneha"), c("Bulk accept"), m("1 d")] },
      { cells: [c("Reject IMS records", "38 records"), m("₹1.2 L", "#a86a2c"), c("Rahul"), c("IMS"), m("2 d")] },
      { cells: [c("Claim cross-period ITC", "47 invoices"), m("₹1.4 L"), c("Rahul"), c("Claim"), m("5 d", "#a86a2c")] },
    ],
    viz: {
      kind: "bars",
      title: "AWAITING SIGN-OFF BY TYPE",
      hint: "threshold ₹50,000",
      items: [
        { label: "Write-off", value: "₹3.17 L · 1", pct: "100%", color: "#a13f28" },
        { label: "Claim cross-period", value: "₹1.4 L · 1", pct: "44%", color: "#6b4fa8" },
        { label: "IMS reject", value: "₹1.2 L · 1", pct: "38%", color: "#a86a2c" },
        { label: "Bulk accept", value: "₹0.6 L · 1", pct: "19%", color: "#5b6bb5" },
      ],
    },
    graphNote: "Every approval carries the case evidence with it, so the approver sees the same three-way comparison and citations the preparer saw.",
    related: [rel("Review queue", "queue", "preparer view"), rel("Periods", "periods", "close gate"), rel("Users", "users", "who can approve")],
    copilot: { text: "The ABC write-off is the only item above your delegation threshold that has been waiting more than two days.", action: "Nudge the approver" },
    rowActs: ["Approve", "Return for detail", "Delegate"],
  },

  imports: {
    title: "Imports",
    desc: "Every file and pull that fed this period.",
    primary: "Upload books",
    secondary: "Fetch from portal",
    kpis: [
      { label: "IMPORTS THIS PERIOD", value: "6", sub: "all succeeded", color: "#2f6b4d" },
      { label: "ROWS INGESTED", value: "37,102", sub: "books + 2B + GSTR-1", color: "#1c1b18" },
      { label: "REJECTED ROWS", value: "18", sub: "bad GSTIN format", color: "#a86a2c" },
      { label: "LAST IMPORT", value: "15 Aug", sub: "09:38", color: "#1c1b18" },
    ],
    grid: "1.4fr 120px 110px 110px 120px",
    cols: ["FILE / SOURCE", "TYPE", "ROWS", "IMPORTED", "STATE"],
    rows: [
      { cells: [c("books_aug.xlsx", "uploaded by Sneha"), m("BOOKS"), m("12,482"), m("15 Aug 09:38"), m("OK", "#2f6b4d")] },
      { cells: [c("GSTR2B_AUG_2026.json", "GST portal pull"), m("GSTR-2B"), m("12,197"), m("14 Aug 22:10"), m("OK", "#2f6b4d")] },
      { cells: [c("GSTR1_AUG_2026.json", "GST portal pull"), m("GSTR-1"), m("12,306"), m("14 Aug 22:12"), m("OK", "#2f6b4d")] },
      { cells: [c("IMS_records_aug.json", "GST portal pull"), m("IMS"), m("12,197"), m("14 Aug 22:14"), m("OK", "#2f6b4d")] },
      { cells: [c("grn_aug.csv", "ERP export"), m("GOODS RECEIPT"), m("9,884"), m("15 Aug 07:02"), m("18 REJECTED", "#a86a2c")] },
    ],
    viz: {
      kind: "flow",
      title: "THIS PERIOD'S LOADS",
      hint: "each load is kept as its own version, never overwritten",
      chains: [
        {
          name: "AUGUST 2026",
          meta: "6 imports, 37,102 rows",
          steps: [
            { label: "GSTR-2B", sub: "14 Aug 22:10", color: "#2f6b4d" },
            { label: "GSTR-1", sub: "14 Aug 22:12", color: "#2f6b4d" },
            { label: "IMS", sub: "14 Aug 22:14", color: "#2f6b4d" },
            { label: "ERP goods receipt", sub: "18 rows rejected", color: "#a86a2c" },
            { label: "Books", sub: "15 Aug 09:38", color: "#2f6b4d" },
          ],
        },
      ],
    },
    graphNote: "An import does not overwrite anything. Each load becomes its own named graph, which is how a case can show what the July file said next to the August file.",
    related: [rel("Sources", "datasources", "connections"), rel("Mappings", "mappings", "column → field"), rel("Periods", "periods", "period state")],
    copilot: { text: "18 goods-receipt rows failed on GSTIN format. All 18 are the same supplier with a trailing space.", action: "Fix and re-import the 18" },
    rowActs: ["View rows", "Re-import", "Roll back load"],
  },

  datasources: {
    title: "Sources",
    desc: "Connections behind the imports.",
    primary: "Add source",
    secondary: "Test connections",
    kpis: [
      { label: "CONNECTED", value: "5", sub: "1 needs attention", color: "#1c1b18" },
      { label: "AUTO-SYNC", value: "4", sub: "daily 22:00", color: "#2f6b4d" },
      { label: "FAILED 30D", value: "2", sub: "both retried", color: "#a86a2c" },
      { label: "CLIENT GSTINS", value: "3", sub: "under this login", color: "#1c1b18" },
    ],
    grid: "1.3fr 130px 130px 130px 110px",
    cols: ["SOURCE", "TYPE", "SCHEDULE", "LAST RUN", "STATE"],
    rows: [
      { cells: [c("GST Portal — filings"), m("API"), c("Daily 22:00"), m("14 Aug 22:10"), m("HEALTHY", "#2f6b4d")] },
      { cells: [c("GST Portal — IMS"), m("API"), c("Daily 22:00"), m("14 Aug 22:14"), m("HEALTHY", "#2f6b4d")] },
      { cells: [c("Tally books export"), m("FILE DROP"), c("Manual"), m("15 Aug 09:38"), m("HEALTHY", "#2f6b4d")] },
      { cells: [c("ERP goods receipt"), m("DATABASE"), c("Daily 07:00"), m("15 Aug 07:02"), m("DEGRADED", "#a86a2c")] },
      { cells: [c("Vendor master"), m("DATABASE"), c("Weekly"), m("11 Aug"), m("HEALTHY", "#2f6b4d")] },
    ],
    viz: {
      kind: "bars",
      title: "SYNC RELIABILITY · 30 DAYS",
      hint: "a degraded source silently ages every case built on it",
      items: [
        { label: "GST Portal — filings", value: "100%", pct: "100%", color: "#2f6b4d" },
        { label: "GST Portal — IMS", value: "97%", pct: "97%", color: "#2f6b4d" },
        { label: "Tally books export", value: "93%", pct: "93%", color: "#2f6b4d" },
        { label: "ERP goods receipt", value: "71%", pct: "71%", color: "#a86a2c" },
        { label: "Vendor master", value: "88%", pct: "88%", color: "#a86a2c" },
      ],
    },
    graphNote: "Reco does not store its own copy of supplier identity. It reads the resolved entity from the graph, so a vendor master fix propagates to every open case.",
    related: [rel("Imports", "imports", "runs"), rel("Mappings", "mappings", "field mapping"), rel("Suppliers", "suppliers", "resolved entities")],
    copilot: { text: "ERP goods receipt has missed 29% of its syncs this month, which is why 14 eligibility flags could not be confirmed.", action: "Open a ticket with IT" },
    rowActs: ["Sync now", "Test connection", "Pause"],
  },

  mappings: {
    title: "Mappings",
    desc: "How your columns become the fields the engine reconciles on.",
    primary: "Save mapping",
    secondary: "Auto-detect",
    kpis: [
      { label: "MAPPED FIELDS", value: "28", sub: "of 31 required", color: "#a86a2c" },
      { label: "UNMAPPED", value: "3", sub: "optional fields", color: "#6f6b62" },
      { label: "TRANSFORMS", value: "6", sub: "trim, upper, date parse", color: "#1c1b18" },
      { label: "REUSED FROM", value: "July", sub: "template applied", color: "#1c1b18" },
    ],
    grid: "1.2fr 1.2fr 130px 1fr",
    cols: ["YOUR COLUMN", "ONTOLOGY FIELD", "TRANSFORM", "SAMPLE"],
    rows: [
      { cells: [c("Party GSTIN"), m("gst:Supplier.hasGSTIN", "#41508f"), c("upper, trim"), m("29XXXXX4321X1Z9")] },
      { cells: [c("Bill No"), m("gst:Invoice.documentNumber", "#41508f"), c("trim"), m("INV-1025")] },
      { cells: [c("Bill Date"), m("gst:Invoice.documentDate", "#41508f"), c("parse DD/MM/YYYY"), m("05/08/2026")] },
      { cells: [c("Assessable Value"), m("gst:Invoice.taxableValue", "#41508f"), c("decimal 2"), m("200000.00")] },
      { cells: [c("IGST Amt"), m("gst:Invoice.igstAmount", "#41508f"), c("decimal 2"), m("38000.00")] },
      { cells: [c("GRN Date"), m("gst:GoodsReceipt.occurredOn", "#41508f"), c("parse"), m("18/08/2026")] },
    ],
    viz: {
      kind: "bars",
      title: "MAPPING COMPLETENESS",
      hint: "28 of 31 required fields mapped",
      items: [
        { label: "Mapped and validated", value: "24", pct: "77%", color: "#2f6b4d" },
        { label: "Mapped, needs check", value: "4", pct: "13%", color: "#a86a2c" },
        { label: "Unmapped (optional)", value: "3", pct: "10%", color: "#c3bdb2" },
      ],
    },
    graphNote: "Ontology field names come from the GST knowledge pack. When the pack updates, new fields appear here rather than requiring a product release.",
    related: [rel("Imports", "imports", "apply on load"), rel("Rules", "rules", "matching config"), rel("Sources", "datasources", "connections")],
    copilot: { text: "Your GRN Date column maps to a field the GST pack added in 1.4.2. Three optional fields are still unmapped.", action: "Auto-map the 3" },
    rowActs: ["Edit mapping", "Preview transform", "Reset to pack default"],
  },

  rules: {
    title: "Rules",
    desc: "Matching tolerance and reason codes. Changes take effect on the next run.",
    primary: "Save & re-run",
    secondary: "Restore defaults",
    kpis: [
      { label: "ACTIVE RULES", value: "62", sub: "from GST pack 1.4.2", color: "#1c1b18" },
      { label: "CUSTOMISED", value: "7", sub: "by your firm", color: "#41508f" },
      { label: "TOLERANCE", value: "₹10", sub: "per tax head", color: "#1c1b18" },
      { label: "LAST CHANGED", value: "02 Aug", sub: "by Rahul", color: "#6f6b62" },
    ],
    grid: "1.5fr 130px 130px 1fr",
    cols: ["RULE", "SETTING", "SOURCE", "EFFECT"],
    rows: [
      { cells: [c("Tax value tolerance"), m("± ₹10"), c("Customised"), c("96 rounding cases auto-matched")] },
      { cells: [c("Invoice number normalisation"), m("ON"), c("GST pack"), c("Strips prefixes and leading zeros")] },
      { cells: [c("Cross-period window"), m("2 periods"), c("Customised"), c("47 late filings matched forward")] },
      { cells: [c("GSTIN transposition detection"), m("ON"), c("GST pack"), c("9 suggested fixes this period")] },
      { cells: [c("Duplicate detection"), m("GSTIN + amount + date"), c("GST pack"), c("18 flagged, none auto-merged")] },
      { cells: [c("Goods receipt check"), m("ON"), c("GST pack 1.4.2"), c("14 timing flags raised")] },
    ],
    viz: {
      kind: "bars",
      title: "CASES AVOIDED BY EACH RULE",
      hint: "what your tolerance settings did this period",
      items: [
        { label: "Tax tolerance ± ₹10", value: "96 auto-matched", pct: "100%", color: "#2f6b4d" },
        { label: "Cross-period window", value: "47 matched forward", pct: "49%", color: "#6b4fa8" },
        { label: "Invoice normalisation", value: "31 matched", pct: "32%", color: "#5b6bb5" },
        { label: "GSTIN transposition", value: "9 suggested", pct: "9%", color: "#a86a2c" },
      ],
    },
    graphNote: "Rules are pack-shipped queries over the graph. A firm-level override is recorded as a customisation, so an upgrade never silently changes your matching behaviour.",
    related: [rel("Mappings", "mappings", "fields"), rel("Exceptions", "exceptions", "resulting cases"), rel("Assistants", "agents", "automation")],
    copilot: { text: "Widening tax tolerance from ₹10 to ₹25 would auto-match 41 more cases worth ₹0.3 L, and hide 2 real mismatches.", action: "Simulate ₹25 tolerance" },
    rowActs: ["Edit rule", "Simulate on August", "Restore default"],
  },

  gstins: {
    title: "GSTINs",
    desc: "Registrations under this client, each with its own filing calendar.",
    primary: "Add GSTIN",
    secondary: "Import list",
    kpis: [
      { label: "REGISTRATIONS", value: "3", sub: "2 states", color: "#1c1b18" },
      { label: "FILING DUE", value: "20 Sep", sub: "GSTR-3B August", color: "#a86a2c" },
      { label: "OPEN EXCEPTIONS", value: "38", sub: "across all", color: "#a86a2c" },
      { label: "ITC AT RISK", value: "₹12.4 L", sub: "combined", color: "#a13f28" },
    ],
    grid: "1fr 1.2fr 116px 116px 130px",
    cols: ["GSTIN", "ENTITY", "STATE", "EXCEPTIONS", "3B DUE"],
    rows: [
      { cells: [m("27XXXXX1234X1Z5"), c("ABC Manufacturing Pvt Ltd"), c("Maharashtra"), m("24", "#a86a2c"), m("20 Sep")] },
      { cells: [m("24XXXXX1234X1Z8"), c("ABC Manufacturing Pvt Ltd"), c("Gujarat"), m("11", "#a86a2c"), m("20 Sep")] },
      { cells: [m("29XXXXX1234X1Z2"), c("ABC Logistics LLP"), c("Karnataka"), m("3"), m("20 Sep")] },
    ],
    viz: {
      kind: "bars",
      title: "EXPOSURE BY REGISTRATION",
      hint: "three registrations, one corporate group in the graph",
      items: [
        { label: "27 · Maharashtra", value: "₹8.1 L · 24 cases", pct: "100%", color: "#a13f28" },
        { label: "24 · Gujarat", value: "₹3.6 L · 11 cases", pct: "44%", color: "#a86a2c" },
        { label: "29 · Karnataka", value: "₹0.7 L · 3 cases", pct: "9%", color: "#5b6bb5" },
      ],
    },
    graphNote: "The graph knows these three registrations belong to one corporate group, which is how an inter-branch transaction stops looking like an unmatched invoice.",
    related: [rel("Periods", "periods", "per registration"), rel("ITC position", "itc", "combined"), rel("Users", "users", "access")],
    copilot: { text: "The Gujarat registration has 11 open cases and no assigned preparer.", action: "Assign a preparer" },
    rowActs: ["Open registration", "Set filing calendar", "Remove"],
  },

  users: {
    title: "Users",
    desc: "Who can see and decide what.",
    primary: "Invite user",
    secondary: "Roles",
    kpis: [
      { label: "USERS", value: "8", sub: "2 partners", color: "#1c1b18" },
      { label: "APPROVERS", value: "2", sub: "above ₹50,000", color: "#1c1b18" },
      { label: "ACTIVE TODAY", value: "5", sub: "", color: "#2f6b4d" },
      { label: "AUDIT EVENTS", value: "1,204", sub: "this period", color: "#1c1b18" },
    ],
    grid: "1.2fr 130px 1fr 116px 130px",
    cols: ["USER", "ROLE", "SCOPE", "CASES OPEN", "LAST ACTIVE"],
    rows: [
      { cells: [c("Sneha Kulkarni", "sneha@firm.in"), c("Preparer"), c("All GSTINs"), m("88"), m("now", "#2f6b4d")] },
      { cells: [c("Rahul Mehta", "rahul@firm.in"), c("Preparer"), c("27, 24"), m("64"), m("2 h ago")] },
      { cells: [c("Anita Rao", "anita@firm.in"), c("Approver"), c("All GSTINs"), m("9"), m("yesterday")] },
      { cells: [c("Client — finance", "ap@abcmfg.in"), c("Read only"), c("27 only"), m("0"), m("3 d ago")] },
    ],
    viz: {
      kind: "bars",
      title: "WORKLOAD",
      hint: "cases currently assigned",
      items: [
        { label: "Sneha Kulkarni", value: "88 open", pct: "100%", color: "#5b6bb5" },
        { label: "Rahul Mehta", value: "64 open", pct: "73%", color: "#5b6bb5" },
        { label: "Anita Rao", value: "9 approvals", pct: "10%", color: "#6b4fa8" },
        { label: "Unassigned", value: "96 cases", pct: "100%", color: "#a13f28" },
      ],
    },
    graphNote: "Read-only client users see cases and evidence but never the underlying graph console — the engine stays behind the workflow.",
    related: [rel("Approvals", "approvals", "sign-off queue"), rel("GSTINs", "gstins", "scope"), rel("Assistants", "agents", "automation limits")],
    copilot: { text: "96 cases have no owner. Sneha is at 88 open, Rahul at 64.", action: "Distribute the 96" },
    rowActs: ["Edit role", "Reassign cases", "Deactivate"],
  },
};
