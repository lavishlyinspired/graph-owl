/** GSTR-2A/GSTR-1 JSON → `gst:Gstr1Invoice` — Plan 108 Slice 1.
 *
 *  **The field names are checked against a published GSP/GSTN API reference,
 *  not recalled.** Three of them are traps a guess gets wrong:
 *
 *  - `fldtr1` (the date the supplier filed) and `flprdr1` (the period they
 *    filed for) sit at the **supplier** level, not on the invoice — so one
 *    filing date covers every invoice in that block, and reading them off
 *    `inv[]` finds nothing at all.
 *  - Those two do **not** use the same date formats the rest of the document
 *    does: the reference shows `"12-May-20"` and `"Apr-18"`, where invoice
 *    dates are `DD-MM-YYYY`. A parser that assumes one format silently drops
 *    the exact field this class exists to carry.
 *  - The taxable value is **per line item** under `itms[].itm_det`, and one
 *    invoice can carry several rate slabs. Reading the first slab under-reports
 *    every multi-rate invoice, which then shows up as a books-vs-GSTR-1
 *    mismatch that is really an importer bug. */

import { describe, expect, it } from "vitest";
import { GstImportError } from "./gstText";
import { filedDate, filedPeriod, normalize, toTurtle } from "./gstr1";

/** One supplier block, in the shape the reference documents. */
function payload(overrides: Record<string, unknown> = {}) {
  return {
    gstin: "27ABCDE1234F1Z5",
    fp: "072026",
    b2b: [
      {
        ctin: "27AABCU9603R1ZM",
        cfs: "Y",
        fldtr1: "11-08-2026",
        flprdr1: "072026",
        inv: [
          {
            inum: "INV-1001",
            idt: "04-07-2026",
            val: 118000.0,
            pos: "27",
            rchrg: "N",
            inv_typ: "R",
            itms: [{ num: 1, itm_det: { rt: 18, txval: 100000.0, iamt: 18000.0, camt: 0, samt: 0, csamt: 0 } }],
          },
        ],
      },
    ],
    ...overrides,
  };
}

describe("a GSTR-2A/GSTR-1 download becomes what the supplier declared", () => {
  it("reads one invoice out of a supplier block", () => {
    const invoice = normalize(payload())[0]!;

    expect(invoice.supplierGstin).toBe("27AABCU9603R1ZM");
    expect(invoice.invoiceNumber).toBe("INV-1001");
    expect(invoice.invoiceDate).toBe("2026-07-04");
    expect(invoice.taxableValue).toBe("100000.00");
    expect(invoice.taxAmount).toBe("18000.00");
    expect(invoice.reverseCharge).toBe("N");
  });

  /** The whole reason this class exists. Without it "the supplier filed late"
   *  is an inference; with it, it is a fact on the finding. */
  it("carries the supplier's own filing date and period, which live on the supplier block", () => {
    const invoice = normalize(payload())[0]!;

    expect(invoice.filedDate).toBe("2026-08-11");
    expect(invoice.period).toBe("2026-07");
  });

  /** A multi-rate invoice is ordinary, not exotic — one line at 18% and one at
   *  5% is a single invoice. Summing is the only reading that reconciles. */
  it("sums every rate slab on one invoice rather than reading the first", () => {
    const doc = payload();
    doc.b2b[0]!.inv[0]!.itms = [
      { num: 1, itm_det: { rt: 18, txval: 100000.0, iamt: 18000.0, camt: 0, samt: 0, csamt: 0 } },
      { num: 2, itm_det: { rt: 5, txval: 40000.0, iamt: 2000.0, camt: 0, samt: 0, csamt: 0 } },
    ];

    const invoice = normalize(doc)[0]!;

    expect(invoice.taxableValue).toBe("140000.00");
    expect(invoice.taxAmount).toBe("20000.00");
  });

  it("adds the four tax heads into one comparable figure", () => {
    const doc = payload();
    doc.b2b[0]!.inv[0]!.itms = [
      { num: 1, itm_det: { rt: 18, txval: 100000.0, iamt: 0, camt: 9000.0, samt: 9000.0, csamt: 500.0 } },
    ];

    expect(normalize(doc)[0]!.taxAmount).toBe("18500.00");
  });

  it("keeps the trade name when the supplier block carries one", () => {
    const doc = payload();
    (doc.b2b[0] as unknown as Record<string, unknown>).trdnm = "Umbrella Industrial Supplies";

    expect(normalize(doc)[0]!.supplierName).toBe("Umbrella Industrial Supplies");
  });

  it("reads every supplier's every invoice, not just the first of each", () => {
    const doc = payload();
    doc.b2b[0]!.inv.push({
      inum: "INV-1002",
      idt: "09-07-2026",
      val: 112100.0,
      pos: "27",
      rchrg: "N",
      inv_typ: "R",
      itms: [{ num: 1, itm_det: { rt: 18, txval: 95000.0, iamt: 17100.0, camt: 0, samt: 0, csamt: 0 } }],
    });

    expect(normalize(doc).map((i) => i.invoiceNumber)).toEqual(["INV-1001", "INV-1002"]);
  });

  /** A period nobody filed against is a legitimate answer, and must not read
   *  as a broken upload. */
  it("returns nothing for a return with no B2B section", () => {
    expect(normalize({ gstin: "27ABCDE1234F1Z5", fp: "072026" })).toEqual([]);
  });

  /** The failure that matters most: a file that is not a return at all must be
   *  refused, because reading it as an empty one reports every booked invoice
   *  as never filed by its supplier. */
  it("refuses a file that is not a GST return rather than reading it as empty", () => {
    expect(() => normalize({ error: "unauthorized" })).toThrow(GstImportError);
    expect(() => normalize({ error: "unauthorized" })).toThrow(/not a GSTR-1/);
  });

  it("refuses a b2b section that is not a list of suppliers", () => {
    expect(() => normalize({ b2b: "none" })).toThrow(GstImportError);
  });
});

describe("the filing date, in every format the reference actually emits", () => {
  it("accepts the day-first form", () => {
    expect(filedDate("11-08-2026")).toBe("2026-08-11");
  });

  /** The published reference's own example is `"12-May-20"`. A parser that
   *  handles only `DD-MM-YYYY` drops it, and the finding then cannot say when
   *  the supplier filed — which is the one thing it exists to say. */
  it("accepts the month-name form the reference documents", () => {
    expect(filedDate("12-May-20")).toBe("2020-05-12");
    expect(filedDate("04-Sep-2026")).toBe("2026-09-04");
  });

  it("passes ISO through unchanged", () => {
    expect(filedDate("2026-08-11")).toBe("2026-08-11");
  });

  /** A wrong date here does not merely display badly: `GoodsReceiptTiming`
   *  and every provision-in-force lookup compare dates lexicographically, so
   *  a mis-parsed date makes a finding cite the wrong month while looking
   *  authoritative. Refusing is the safe direction. */
  it("refuses a date it cannot place rather than guessing", () => {
    expect(() => filedDate("sometime in May")).toThrow(GstImportError);
    expect(() => filedDate("13-Foo-2026")).toThrow(GstImportError);
  });

  it("treats an absent filing date as absent, not as today", () => {
    expect(filedDate("")).toBe("");
    expect(filedDate(undefined)).toBe("");
  });
});

describe("the filing period, in every format the reference actually emits", () => {
  it("accepts MMYYYY", () => {
    expect(filedPeriod("072026")).toBe("2026-07");
  });

  it("accepts the month-name form the reference documents", () => {
    expect(filedPeriod("Apr-18")).toBe("2018-04");
    expect(filedPeriod("Jul-2026")).toBe("2026-07");
  });

  it("refuses a period it cannot place", () => {
    expect(() => filedPeriod("132026")).toThrow(GstImportError);
    expect(() => filedPeriod("quarter 1")).toThrow(GstImportError);
  });
});

describe("the Turtle a pack rule cannot tell from a hand-written fixture", () => {
  it("writes the declared class, not the 2B one", () => {
    const turtle = toTurtle(normalize(payload()));

    expect(turtle).toContain("gst:g1-INV-1001 rdf:type gst:Gstr1Invoice");
    expect(turtle).not.toContain("gst:Gstr2bInvoice");
  });

  it("writes the filing date as its own predicate", () => {
    expect(toTurtle(normalize(payload()))).toContain('gst:filedDate     "2026-08-11"');
  });

  /** One `gst:Supplier` subject per GSTIN, pointed at by `issuedBy` — the
   *  same shape `gstr2b.ts` emits, so a third source enriches the *same* node
   *  rather than creating a fourth copy of a GSTIN string. */
  it("makes the supplier a subject and reaches it by an edge", () => {
    const turtle = toTurtle(normalize(payload()));

    expect(turtle).toContain("gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier");
    expect(turtle).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM");

    const invoiceBlock = turtle.slice(turtle.indexOf("gst:g1-INV-1001"));
    expect(invoiceBlock).not.toContain("gst:supplierGstin");
  });

  it("writes one supplier subject however many invoices they filed", () => {
    const doc = payload();
    doc.b2b[0]!.inv.push({
      inum: "INV-1002",
      idt: "09-07-2026",
      val: 112100.0,
      pos: "27",
      rchrg: "N",
      inv_typ: "R",
      itms: [{ num: 1, itm_det: { rt: 18, txval: 95000.0, iamt: 17100.0, camt: 0, samt: 0, csamt: 0 } }],
    });

    const turtle = toTurtle(normalize(doc));

    expect((turtle.match(/rdf:type gst:Supplier/g) ?? []).length).toBe(1);
  });

  /** "Not reported" and "reported as empty" are different facts, and a
   *  reconciliation is mostly a set of questions about missing data. */
  it("omits a field the return did not carry rather than writing it blank", () => {
    const doc = payload();
    delete (doc.b2b[0] as unknown as Record<string, unknown>).fldtr1;

    const turtle = toTurtle(normalize(doc));

    expect(turtle).not.toContain("gst:filedDate");
    expect(turtle).toContain("gst:g1-INV-1001 rdf:type gst:Gstr1Invoice");
  });

  it("escapes a supplier name that would otherwise corrupt every triple after it", () => {
    const doc = payload();
    (doc.b2b[0] as unknown as Record<string, unknown>).trdnm = 'Acme "Best" \\ Co';

    expect(toTurtle(normalize(doc))).toContain('gst:supplierName  "Acme \\"Best\\" \\\\ Co"');
  });
});
