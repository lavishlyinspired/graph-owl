import { describe, expect, it } from "vitest";
import source from "./ReconciliationWorkspace.tsx?raw";

/** The "16 invoices" figure on a source card used to be a plain `<Text>`: a
 *  number with nothing behind it. It is now a button that opens the invoices
 *  themselves. A structural test, because `SourceCard` is not exported and the
 *  page only renders against a real graph — the same `?raw` technique
 *  `ReviewQueue.structural.test.ts` established, for the same reason: a unit
 *  test of the visible output cannot tell a real modal from a figure that
 *  happens to agree with it. */

describe("a source card's invoice count opens the invoices behind it", () => {
  it("renders the count as a trigger, not a plain figure", () => {
    expect(source).toMatch(/type="link"/);
    expect(source).toMatch(/setInvoiceOpen\(true\)/);
  });

  it("opens the source's own rows in a Modal table", () => {
    expect(source).toMatch(/<Modal/);
    expect(source).toMatch(/open=\{invoiceOpen\}/);
    expect(source).toMatch(/dataSource=\{\[\.\.\.rows\]\}/);
    expect(source).toMatch(/columns=\{invoiceColumns\(money\)\}/);
  });

  it("keeps the taxable figure out of the trigger", () => {
    const trigger = source.match(
      /type="link"[\s\S]*?setInvoiceOpen\(true\)/,
    )?.[0];
    expect(trigger).toBeDefined();
    expect(trigger).not.toMatch(/taxable/);
  });
});

/** The invoice popup is a working paper a CA reads, not a raw dump: head-wise
 *  columns because ITC is claimed head by head, a scroll rather than pages
 *  because a total belongs under its invoices, and a totals row because a
 *  table of ₹ values that does not total them is not a table of ₹ values. */
describe("the invoices behind a source card read head-wise, scrolled, and totalled", () => {
  it("shows the tax split into heads, not one lump", () => {
    const columns = source.match(/function invoiceColumns[\s\S]*?\n\}/)?.[0];
    expect(columns).toBeDefined();
    expect(columns).toMatch(/"IGST"/);
    expect(columns).toMatch(/"CGST"/);
    expect(columns).toMatch(/"SGST"/);
    expect(columns).toMatch(/"Cess"/);
  });

  it("scrolls the table instead of paging it, and totals the source's own rows", () => {
    const modal = source.match(/<Modal[\s\S]*?<\/Modal>/)?.[0];
    expect(modal).toBeDefined();
    expect(modal).toMatch(/pagination=\{false\}/);
    expect(modal).toMatch(/scroll=\{\{ x: "max-content", y: 360 \}\}/);
    expect(modal).toMatch(/summary=\{invoiceTotals\(summary, money\)\}/);
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

describe("the run-the-rules section opens each registered rule", () => {
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
