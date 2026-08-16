import { describe, expect, it } from "vitest";
import source from "./ReconciliationWorkspace.tsx?raw";

/** Plan 120 Slice E — reco-now is the only place upload and manual
 *  reconciliation-running happen now; this page stays a *read-only*
 *  statement view over whatever is already in the graph. Structural, for
 *  the same reason every other test in this file is: the page only
 *  renders meaningfully against a real backend, so a unit render cannot
 *  prove an upload control was never mounted — its absence from the
 *  source is the only thing a test can actually pin. */
describe("this page offers no way to upload or manually run reconciliation", () => {
  it("has no upload surface — reco-now owns that workflow", () => {
    expect(source).not.toMatch(/Upload\.Dragger/);
    expect(source).not.toMatch(/importThroughSurface/);
  });

  it("has no manual run trigger — reco-now already triggers it automatically on upload", () => {
    expect(source).not.toMatch(/api\.reconcilePack/);
    expect(source).not.toMatch(/SyncOutlined/);
  });

  it("still refreshes from the graph on mount, so the statement is never empty just because nobody clicked a button here", () => {
    expect(source).toMatch(/void refresh\(\)/);
    expect(source).toMatch(/api\.sparql\(/);
  });

  it("keeps the read-only actions a run-free page still needs: export, open in review, and what the rules are", () => {
    expect(source).toMatch(/exportCsv/);
    expect(source).toMatch(/openInReview/);
    expect(source).toMatch(/rulesTrigger/);
  });
});

/** C2 — the period filter narrows the reconciliation to one filing period, so
 *  a CA works July and August against their own rows rather than a statement
 *  mixing both. Structural, like the rest of this file: the Select only
 *  matters when the workspace renders against a real graph, and the *logic* it
 *  feeds is unit-tested in `statement.test.ts` — this pins the wiring. */
describe("the GST period filter narrows the workspace", () => {
  it("offers a period Select and feeds narrowed rows to the statement", () => {
    expect(source).toMatch(/\bSelect\b/);
    expect(source).toMatch(/setPeriod/);
    expect(source).toMatch(/forPeriod/);
    expect(source).toMatch(/findingsForPeriod/);
  });
});

/** The invoice popup behind a "what the graph knows" tile is a working paper
 *  a CA reads, not a raw dump: head-wise columns because ITC is claimed head
 *  by head, a scroll rather than pages because a total belongs under its
 *  invoices, and a totals row because a table of ₹ values that does not
 *  total them is not a table of ₹ values. Plan 120 Slice E moved this off a
 *  per-source upload card and onto the graph-tile modal (`InvoiceRecordsModal`)
 *  — `invoiceColumns`/`invoiceTotals` themselves are unchanged, reused by the
 *  new caller exactly as the old one used them. */
describe("the invoices behind a graph tile read head-wise, scrolled, and totalled", () => {
  it("shows the tax split into heads, not one lump", () => {
    const columns = source.match(/function invoiceColumns[\s\S]*?\n\}/)?.[0];
    expect(columns).toBeDefined();
    expect(columns).toMatch(/"IGST"/);
    expect(columns).toMatch(/"CGST"/);
    expect(columns).toMatch(/"SGST"/);
    expect(columns).toMatch(/"Cess"/);
  });

  it("scrolls the table instead of paging it, and totals every source's rows together", () => {
    const modal = source.match(/function InvoiceRecordsModal[\s\S]*?\n\}\n\n(?=function |export function)/)?.[0];
    expect(modal).toBeDefined();
    expect(modal).toMatch(/pagination=\{false\}/);
    expect(modal).toMatch(/scroll=\{\{ x: "max-content", y: 320 \}\}/);
    expect(modal).toMatch(/summary=\{invoiceTotals\(sourceSummary\(flat\), money, 1, 6\)\}/);
  });

  it("totals every head the columns show", () => {
    const totals = source.match(/function invoiceTotals[\s\S]*?\n\}/)?.[0];
    expect(totals).toBeDefined();
    expect(totals).toMatch(/<Table.Summary.Row>/);
    expect(totals).toMatch(/money\(summary\.taxableValue\)/);
    expect(totals).toMatch(/money\(summary\.igst\)/);
    expect(totals).toMatch(/money\(summary\.cgst\)/);
    expect(totals).toMatch(/money\(summary\.sgst\)/);
    expect(totals).toMatch(/money\(summary\.cess\)/);
    expect(totals).toMatch(/money\(summary\.taxAmount\)/);
  });
});

describe("the statement section opens an explanation of how its figures arise", () => {
  it("offers a trigger next to the heading", () => {
    expect(source).toMatch(/setStatementHelpOpen\(true\)/);
    expect(source).toMatch(/statementHelpOpen/);
  });

  it("explains the books taxable figure as the sum of the loaded invoices", () => {
    const help = source.match(/function StatementHelp[\s\S]*?\n\}\n\n(?=function |export function)/)?.[0];
    expect(help).toBeDefined();
    expect(help).toMatch(/taxable values of the invoices/);
    expect(help).toMatch(/statement\.books\.count/);
  });
});

describe("the read-only actions toolbar opens each registered rule", () => {
  it("fetches the registered rules and opens them from the section", () => {
    expect(source).toMatch(/setRulesOpen\(true\)/);
    expect(source).toMatch(/rulesOpen/);
    expect(source).toMatch(/api\.findingRules/);
  });

  it("renders each rule's summary, citation and next action", () => {
    const panel = source.match(/function RulesPanel[\s\S]*?\n\}\n\n(?=function |export function)/)?.[0];
    expect(panel).toBeDefined();
    expect(panel).toMatch(/rule\.summary/);
    expect(panel).toMatch(/rule\.governedBy/);
    expect(panel).toMatch(/scenario\.meaning/);
    expect(panel).toMatch(/scenario\.nextAction/);
  });
});

describe("what-the-graph-knows tiles open the data behind them", () => {
  it("makes every tile open something", () => {
    expect(source).toMatch(/hoverable/);
    expect(source).toMatch(/setGraphTileOpen/);
  });

  it("breaks the invoice records down per source", () => {
    const modal = source.match(/function InvoiceRecordsModal[\s\S]*?\n\}\n\n(?=function |export function)/)?.[0];
    expect(modal).toBeDefined();
    expect(modal).toMatch(/sourceKey/);
    expect(modal).toMatch(/invoiceColumns\(money\)/);
  });

  it("lists the named graphs with what each holds", () => {
    const modal = source.match(/function NamedGraphsModal[\s\S]*?\n\}\n\n(?=function |export function)/)?.[0];
    expect(modal).toBeDefined();
    expect(modal).toMatch(/count/);
  });
});
