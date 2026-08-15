/** The browser importer, pinned to the same expectations as the Python one.
 *
 *  **These assertions are the whole justification for the duplication.** The
 *  same fixture, the same expected values. If the two implementations drift,
 *  this file fails — which is the only thing that makes two copies of a
 *  normalizer defensible. */

import { describe, expect, it } from "vitest";
import { Gstr2bError, isoDate, normalize, returnPeriod, toTurtle } from "./gstr2b";

function getPayload(overrides: Record<string, unknown> = {}) {
  return {
    data: {
      data: {
        docdata: {
          b2b: [
            {
              ctin: "27AABCU9603R1ZM",
              trdnm: "Umbrella Supplies",
              supprd: "072026",
              inv: [
                { inum: "INV-1001", dt: "04-07-2026", txval: 100000.0, igst: 18000.0, cgst: 0, sgst: 0, cess: 0, itcavl: "Y", rev: "N", typ: "R", pos: "27" },
                { inum: "INV-1005", dt: "24-07-2026", txval: 40000.0, igst: 0, cgst: 3600.0, sgst: 3600.0, cess: 0, itcavl: "N", rev: "N", typ: "R", pos: "27" },
              ],
            },
            {
              ctin: "29AACCG0527D1Z8",
              supprd: "072026",
              inv: [{ inum: "INV-1003", dt: "15-07-2026", txval: 250000.0, igst: 45000.0, cgst: 0, sgst: 0, cess: 0, itcavl: "Y", rev: "N", typ: "R", pos: "29" }],
            },
          ],
        },
      },
    },
    ...overrides,
  };
}

describe("normalize", () => {
  it("walks every supplier and every invoice", () => {
    expect(normalize(getPayload()).map((i) => i.invoiceNumber)).toEqual(["INV-1001", "INV-1005", "INV-1003"]);
  });

  it("converts DD-MM-YYYY to ISO, because the rules depend on that ordering", () => {
    // Passing 04-07-2026 through makes the string sort by day-of-month, which
    // silently resolves the wrong Rule 36(4) provision.
    expect(normalize(getPayload())[0]!.invoiceDate).toBe("2026-07-04");
  });

  it("sums the four tax components into the figure the register compares against", () => {
    const intraState = normalize(getPayload())[1]!;

    expect(intraState.taxAmount).toBe("7200.00");
    expect(intraState.cgst).toBe("3600.00");
    expect(intraState.sgst).toBe("3600.00");
  });

  it("keeps money at two decimals so string comparison against the register works", () => {
    expect(normalize(getPayload())[1]!.taxableValue).toBe("40000.00");
  });

  it("derives the period from the document's own declared return period, not the invoice date", () => {
    // An invoice dated in July can legitimately belong to August's GSTR-2B —
    // that's the whole carry-forward scenario `supprd` exists to represent.
    // Deriving from invoiceDate instead would make this untestable.
    const payload = getPayload();
    payload.data.data.docdata.b2b[0]!.supprd = "082026";

    expect(normalize(payload)[0]!.period).toBe("2026-08");
  });

  it("scopes the declared return period to its own supplier, not every invoice", () => {
    const payload = getPayload();
    payload.data.data.docdata.b2b[0]!.supprd = "082026"; // Umbrella files for August
    // Globex (b2b[1]) keeps the factory default of "072026".

    const invoices = normalize(payload);

    expect(invoices[0]!.period).toBe("2026-08"); // INV-1001, Umbrella
    expect(invoices[1]!.period).toBe("2026-08"); // INV-1005, Umbrella
    expect(invoices[2]!.period).toBe("2026-07"); // INV-1003, Globex
  });

  it("refuses a supplier block with no declared return period", () => {
    // Silently falling back to the invoice date would reintroduce the exact
    // bug this field exists to close, invisibly.
    const payload = getPayload();
    delete (payload.data.data.docdata.b2b[0] as { supprd?: string }).supprd;

    expect(() => normalize(payload)).toThrow(Gstr2bError);
    // The exact quoted value, not just the field name — otherwise a defaulted
    // placeholder string could stand in for the missing field unnoticed.
    expect(() => normalize(payload)).toThrow("'' is not a return period");
  });

  it("accepts a portal download and a GSP response identically", () => {
    const inner = getPayload().data.data;

    expect(normalize({ data: inner })).toEqual(normalize(getPayload()));
    expect(normalize(inner)).toEqual(normalize(getPayload()));
  });

  it("treats a period nobody filed against as empty rather than an error", () => {
    expect(normalize({ docdata: {} })).toEqual([]);
  });

  it("refuses a file that is not a GSTR-2B download", () => {
    // An error page or the wrong export read as "no invoices" would report
    // every claimed invoice as unmatched.
    expect(() => normalize({ error: "unauthorized" })).toThrow(Gstr2bError);
    expect(() => normalize({})).toThrow(/docdata/);
  });
});

describe("returnPeriod", () => {
  it("converts the authority's MMYYYY to the rules' YYYY-MM", () => {
    expect(returnPeriod("072026")).toBe("2026-07");
  });

  it("accepts month boundaries 01 and 12", () => {
    expect(returnPeriod("012026")).toBe("2026-01");
    expect(returnPeriod("122026")).toBe("2026-12");
  });

  it("refuses a month of 00 or 13, rather than emitting an unsortable period", () => {
    expect(() => returnPeriod("002026")).toThrow(Gstr2bError);
    expect(() => returnPeriod("132026")).toThrow(Gstr2bError);
  });

  it("refuses anything that is not six digits", () => {
    expect(() => returnPeriod("2026-07")).toThrow(Gstr2bError);
    expect(() => returnPeriod("")).toThrow(Gstr2bError);
  });

  it("refuses extra characters before or after the six digits, not just a match inside them", () => {
    // An unanchored match would find "072026" inside either string and
    // silently accept garbage on one side of it.
    expect(() => returnPeriod("X072026")).toThrow(Gstr2bError);
    expect(() => returnPeriod("072026X")).toThrow(Gstr2bError);
  });
});

describe("isoDate", () => {
  it("passes an ISO date through rather than re-reading it day-first", () => {
    expect(isoDate("2026-07-04")).toBe("2026-07-04");
  });

  it("accepts slash-separated GST dates", () => {
    expect(isoDate("04/07/2026")).toBe("2026-07-04");
  });

  it("refuses a date it cannot place rather than guessing", () => {
    expect(() => isoDate("July 4th")).toThrow(/not a date/);
    expect(() => isoDate("")).toThrow();
  });
});

describe("toTurtle", () => {
  it("emits the vocabulary the pack fixtures already use", () => {
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toContain("gst:2b-INV-1001 rdf:type gst:Gstr2bInvoice");
    expect(turtle).toContain('gst:invoiceDate   "2026-07-04"');
    expect(turtle).toContain('gst:itcAvailable  "N"');
    expect(turtle).toContain('gst:period        "2026-07"');
  });

  it("omits an absent value rather than writing it blank", () => {
    // "not reported" and "reported as empty" are different facts, and a
    // reconciliation is mostly questions about missing data.
    const turtle = toTurtle(normalize(getPayload()));
    const globex = turtle.slice(turtle.indexOf("2b-INV-1003"));

    expect(globex).not.toContain("supplierName");
  });

  it("escapes a quote or backslash so one bad supplier cannot corrupt the document", () => {
    const payload = getPayload();
    payload.data.data.docdata.b2b[0]!.trdnm = 'The "Best" Co \\ Ltd';

    const turtle = toTurtle(normalize(payload));

    expect(turtle).toContain('\\"Best\\"');
    expect(turtle).toContain("\\\\");
  });

  it("terminates the last predicate of each subject with a period", () => {
    // A stray semicolon on the final line makes the whole document unparseable
    // and the import fails wholesale rather than per-row. The canonical
    // Invoice subject's own last line is the one checked here — Plan 109
    // Slice 2 moved `period` off being the invoice's own last field.
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toMatch(/gst:reflectedIn {3}gst:2b-INV-1001 \./);
  });

  // ---- Supplier as a real graph node, not a literal on the invoice ----
  //
  // The gap `plans/105c-gst-causal-graph.md` names directly: `gst:Supplier`
  // was declared and never instantiated. Pinned to the same shape the
  // Python port (`connectors/python/graph_owl_packs/gstr2b.py`) now emits.

  it("gives each unique supplier its own subject", () => {
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toContain("gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier");
    expect(turtle).toContain("gst:supplier-29AACCG0527D1Z8 rdf:type gst:Supplier");
    expect((turtle.match(/rdf:type gst:Supplier/g) ?? []).length).toBe(2);
  });

  it("carries the GSTIN and name on the supplier subject, not the invoice", () => {
    const turtle = toTurtle(normalize(getPayload()));
    const supplierBlock = turtle.slice(turtle.indexOf("gst:supplier-27AABCU9603R1ZM"));

    expect(supplierBlock).toContain('gst:supplierGstin "27AABCU9603R1ZM"');
    expect(supplierBlock).toContain('gst:supplierName  "Umbrella Supplies"');
  });

  it("points an invoice at its supplier by edge, not literal", () => {
    const turtle = toTurtle(normalize(getPayload()));
    const start = turtle.indexOf("gst:2b-INV-1001");
    const invoiceBlock = turtle.slice(start, turtle.indexOf("\n\n", start));

    expect(invoiceBlock).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM");
    expect(invoiceBlock).not.toContain("gst:supplierGstin");
    expect(invoiceBlock).not.toContain("gst:supplierName");
  });

  it("resolves two invoices from the same supplier to the same subject", () => {
    const turtle = toTurtle(normalize(getPayload()));
    const firstStart = turtle.indexOf("gst:2b-INV-1001");
    const first = turtle.slice(firstStart, turtle.indexOf("\n\n", firstStart));
    const secondStart = turtle.indexOf("gst:2b-INV-1005");
    const second = turtle.slice(secondStart, turtle.indexOf("\n\n", secondStart));

    expect(first).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM");
    expect(second).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM");
  });
});

describe("the Gstr2bStatement and canonical gst:Invoice — Plan 109 Slice 2", () => {
  it("emits one Gstr2bStatement per period, generated for the single well-known Recipient", () => {
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toContain("gst:g2bstatement-2026-07 rdf:type gst:Gstr2bStatement");
    expect(turtle).toContain("gst:recipient-self rdf:type gst:Recipient");
    const statementBlock = turtle.slice(turtle.indexOf("gst:g2bstatement-2026-07"));
    expect(statementBlock).toContain("gst:generatedFor  gst:recipient-self");
    expect((turtle.match(/rdf:type gst:Gstr2bStatement/g) ?? []).length).toBe(1);
  });

  it("points each per-line record at its statement with reflectedIn, not period directly", () => {
    const turtle = toTurtle(normalize(getPayload()));
    const start = turtle.indexOf("gst:2b-INV-1001");
    const invoiceBlock = turtle.slice(start, turtle.indexOf("\n\n", start));

    expect(invoiceBlock).toContain("gst:reflectedIn   gst:g2bstatement-2026-07");
  });

  it("emits a canonical gst:Invoice subject with reflectedIn to the per-line record", () => {
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toContain("gst:invoice-27AABCU9603R1ZM-INV1001 rdf:type gst:Invoice");
    const canonicalStart = turtle.indexOf("gst:invoice-27AABCU9603R1ZM-INV1001");
    const canonicalBlock = turtle.slice(canonicalStart, turtle.indexOf("\n\n", canonicalStart));
    expect(canonicalBlock).toContain("gst:reflectedIn   gst:2b-INV-1001");
    expect(canonicalBlock).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM ;");
    expect(canonicalBlock).not.toContain('"gst:supplier-27AABCU9603R1ZM"');
  });

  /** **Every subject is its own blank-line-separated block — one `rdf:type`
   *  declaration each.** Catches a blank-line separator silently replaced by
   *  stray content, which would merge two subjects into one unparseable
   *  block or leave a bogus fragment neither block claims — the same
   *  "unparseable document" failure mode the very first `toTurtle` test in
   *  this file guards for on the register side.
   */
  it("separates every subject with a genuine blank line, never merging two into one block", () => {
    const turtle = toTurtle(normalize(getPayload()));

    for (const block of turtle.split("\n\n")) {
      expect((block.match(/rdf:type/g) ?? []).length).toBeLessThanOrEqual(1);
    }
    // Ten subjects — two suppliers, one recipient, one statement, three
    // per-line invoices, three canonical invoices — each its own block.
    expect((turtle.match(/rdf:type/g) ?? []).length).toBe(10);
  });

  it("emits two different statements for two different periods", () => {
    const payload = getPayload();
    payload.data.data.docdata.b2b[1]!.supprd = "082026";

    const turtle = toTurtle(normalize(payload));

    expect(turtle).toContain("gst:g2bstatement-2026-07 rdf:type gst:Gstr2bStatement");
    expect(turtle).toContain("gst:g2bstatement-2026-08 rdf:type gst:Gstr2bStatement");
    expect((turtle.match(/rdf:type gst:Gstr2bStatement/g) ?? []).length).toBe(2);
  });

  /** `toTurtle` is a pure function over its own input type, not only over
   *  what `normalize` happens to produce — an invoice with no period at all
   *  must get no Statement, no Recipient, and no `reflectedIn` edge, and the
   *  invoice's own last field must still be correctly period-terminated. */
  it("emits no statement, no recipient and no reflectedIn edge for an invoice with no period", () => {
    const invoice = normalize(getPayload())[0]!;
    const turtle = toTurtle([{ ...invoice, period: "" }]);

    expect(turtle).not.toContain("gst:Gstr2bStatement");
    expect(turtle).not.toContain("gst:Recipient");
    const invoiceStart = turtle.indexOf("gst:2b-INV-1001");
    const invoiceBlock = turtle.slice(invoiceStart, turtle.indexOf("\n\n", invoiceStart));
    expect(invoiceBlock).not.toContain("gst:reflectedIn");
    // The last present field must still terminate with a period, not a
    // stray semicolon left over from an off-by-one total — the same class
    // of bug `terminates the last predicate of each subject` guards above.
    expect(invoiceBlock).toMatch(/"27" \.$/);
  });

  it("still terminates the invoice's own last field with a period, and reflectedIn with its own, when a statement is present", () => {
    const turtle = toTurtle(normalize(getPayload()));
    const invoiceStart = turtle.indexOf("gst:2b-INV-1001");
    const invoiceBlock = turtle.slice(invoiceStart, turtle.indexOf("\n\n", invoiceStart));

    expect(invoiceBlock).toMatch(/"27" ;$/m);
    expect(invoiceBlock).toMatch(/gst:reflectedIn {3}gst:g2bstatement-2026-07 \.$/);
    // **The reflectedIn line is unconditionally period-terminated, on its
    // own — so `total`'s only remaining job is making sure no *literal*
    // field ever wrongly picks up that period instead.** An off-by-one
    // `total` (e.g. excluding the trailing `reflectedIn` slot) makes an
    // earlier field — `invoiceType`, second-to-last — terminate with a
    // period mid-list, which is exactly the "stray semicolon/period makes
    // the whole document unparseable" failure this file's very first
    // `toTurtle` test already guards for the register side.
    const periodTerminatedLines = (invoiceBlock.match(/ \.$/gm) ?? []).length;
    expect(periodTerminatedLines).toBe(1);
  });
});
