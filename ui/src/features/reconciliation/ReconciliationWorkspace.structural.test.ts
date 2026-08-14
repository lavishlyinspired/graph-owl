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
