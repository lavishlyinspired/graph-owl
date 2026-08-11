/** The browser importer, pinned to the same expectations as the Python one.
 *
 *  **These assertions are the whole justification for the duplication.** The
 *  same fixture, the same expected values. If the two implementations drift,
 *  this file fails — which is the only thing that makes two copies of a
 *  normalizer defensible. */

import { describe, expect, it } from "vitest";
import { Gstr2bError, isoDate, normalize, toTurtle } from "./gstr2b";

function getPayload(overrides: Record<string, unknown> = {}) {
  return {
    data: {
      data: {
        docdata: {
          b2b: [
            {
              ctin: "27AABCU9603R1ZM",
              trdnm: "Umbrella Supplies",
              inv: [
                { inum: "INV-1001", dt: "04-07-2026", txval: 100000.0, igst: 18000.0, cgst: 0, sgst: 0, cess: 0, itcavl: "Y", rev: "N", typ: "R", pos: "27" },
                { inum: "INV-1005", dt: "24-07-2026", txval: 40000.0, igst: 0, cgst: 3600.0, sgst: 3600.0, cess: 0, itcavl: "N", rev: "N", typ: "R", pos: "27" },
              ],
            },
            {
              ctin: "29AACCG0527D1Z8",
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

  it("derives the period from the invoice date, not from today", () => {
    expect(normalize(getPayload()).every((i) => i.period === "2026-07")).toBe(true);
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
    // and the import fails wholesale rather than per-row.
    const turtle = toTurtle(normalize(getPayload()));

    expect(turtle).toMatch(/"2026-07" \./);
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
