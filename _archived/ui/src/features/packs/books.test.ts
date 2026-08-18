/** The purchase register, from wherever a CA actually keeps it — Plan 108.
 *
 *  **Without this surface the product only reconciles its own demo data.**
 *  Every finding in this pack compares the taxpayer's books against what the
 *  authority holds, and until now the books could only arrive as a
 *  hand-written Turtle fixture shipped inside the pack. A CA's register lives
 *  in Tally, Busy, Zoho or a spreadsheet, and comes out as CSV.
 *
 *  **Column names are the whole problem, and they are not standardisable.**
 *  The practitioner reconciliation formats this was checked against use `GST
 *  NO.`, `PARTICULARS`, `INV. NO`, `DATE`, `BASIC`; Tally exports say
 *  `Supplier GSTIN`, `Voucher No`, `Taxable Value`. Both are the same six
 *  facts. So the importer matches on a normalized alias rather than an exact
 *  header, and says plainly which column it could not find when it cannot —
 *  an importer that silently reads zero rows reports the entire register as
 *  unbooked, which is the most alarming possible way to fail. */

import { describe, expect, it } from "vitest";
import { GstImportError } from "./gstText";
import { normalize, parseDelimited, toTurtle } from "./books";

const HEADER = "GSTIN,Supplier Name,Invoice No,Invoice Date,Taxable Value,IGST,CGST,SGST,Cess";
const ROW = "27AABCU9603R1ZM,Umbrella Supplies,INV-1001,04-07-2026,100000.00,18000.00,0,0,0";

describe("reading the delimited file itself", () => {
  it("reads a header and its rows", () => {
    expect(parseDelimited("a,b\n1,2")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  /** A supplier called `Kumar, Sons & Co` is ordinary, and splitting on every
   *  comma shifts every column after it — silently producing an invoice number
   *  where a date should be. */
  it("keeps a quoted field containing the delimiter in one piece", () => {
    expect(parseDelimited('a,b\n"Kumar, Sons & Co",2')).toEqual([
      ["a", "b"],
      ["Kumar, Sons & Co", "2"],
    ]);
  });

  it("reads a doubled quote as one literal quote", () => {
    expect(parseDelimited('a\n"Acme ""Best"" Ltd"')).toEqual([["a"], ['Acme "Best" Ltd']]);
  });

  /** A file exported on Windows and opened on a Mac. Leaving the `\r` on turns
   *  the last column of every row into a value that matches nothing. */
  it("survives Windows line endings", () => {
    expect(parseDelimited("a,b\r\n1,2\r\n")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  /** Excel's "Save as Unicode text" is tab-separated and is what a
   *  spreadsheet user gets when a CSV mangles their data. */
  it("reads a tab-separated export", () => {
    expect(parseDelimited("a\tb\n1\t2")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("drops trailing blank lines rather than emitting an empty row", () => {
    expect(parseDelimited("a,b\n1,2\n\n")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });
});

describe("a purchase register becomes what the taxpayer claims", () => {
  it("reads one invoice", () => {
    const invoice = normalize(`${HEADER}\n${ROW}`)[0]!;

    expect(invoice.supplierGstin).toBe("27AABCU9603R1ZM");
    expect(invoice.supplierName).toBe("Umbrella Supplies");
    expect(invoice.invoiceNumber).toBe("INV-1001");
    expect(invoice.invoiceDate).toBe("2026-07-04");
    expect(invoice.taxableValue).toBe("100000.00");
    expect(invoice.taxAmount).toBe("18000.00");
  });

  /** The two column vocabularies this was checked against are both real and
   *  neither is going to change. Matching on an alias is what lets one
   *  importer serve both. */
  it("reads the practitioner's own column names as readily as an ERP's", () => {
    const csv =
      "GST NO.,PARTICULARS,INV. NO,DATE,BASIC,IGST,CGST,SGST\n" +
      "27AABCU9603R1ZM,Umbrella Supplies,INV-1001,04-07-2026,100000.00,18000.00,0,0";

    const invoice = normalize(csv)[0]!;

    expect(invoice.invoiceNumber).toBe("INV-1001");
    expect(invoice.taxableValue).toBe("100000.00");
  });

  it("ignores case, spacing and punctuation in a header", () => {
    const csv = "  gstin  ,supplier_name,voucher no.,bill date,taxable amount\n27AABCU9603R1ZM,X,INV-9,04-07-2026,100";

    expect(normalize(csv)[0]!.invoiceNumber).toBe("INV-9");
  });

  it("adds the four tax heads into one comparable figure", () => {
    const csv = `${HEADER}\n27AABCU9603R1ZM,X,INV-1,04-07-2026,100000,0,9000,9000,500`;

    expect(normalize(csv)[0]!.taxAmount).toBe("18500.00");
  });

  /** A spreadsheet's own thousands separators and currency symbol are not a
   *  user error — they are what "format as currency" does. */
  it("reads a figure a spreadsheet formatted as currency", () => {
    const csv = `${HEADER}\n27AABCU9603R1ZM,X,INV-1,04-07-2026,"₹1,00,000.00","18,000.00",0,0,0`;

    expect(normalize(csv)[0]!.taxableValue).toBe("100000.00");
    expect(normalize(csv)[0]!.taxAmount).toBe("18000.00");
  });

  it("takes an explicit tax total when the export has no head-wise split", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value,Tax Amount\n27AABCU9603R1ZM,INV-1,04-07-2026,100000,18000";

    expect(normalize(csv)[0]!.taxAmount).toBe("18000.00");
  });

  it("carries the reverse-charge flag, which decides whether a rule fires at all", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value,Reverse Charge\n27AABCU9603R1ZM,INV-1,04-07-2026,100000,Y";

    expect(normalize(csv)[0]!.reverseCharge).toBe("Y");
  });

  it("normalises a spelled-out reverse-charge flag to the Y the rules read", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value,RCM\n27AABCU9603R1ZM,INV-1,04-07-2026,100000,Yes";

    expect(normalize(csv)[0]!.reverseCharge).toBe("Y");
  });

  /** **Unlike GSTR-2B, the register's period genuinely is the invoice's own
   *  month** — a purchase is booked when it is booked, and there is no filing
   *  to lag behind. An explicit column still wins, because a taxpayer who
   *  books a July invoice in August knows something the date does not say. */
  it("derives the period from the invoice date, and prefers an explicit column", () => {
    expect(normalize(`${HEADER}\n${ROW}`)[0]!.period).toBe("2026-07");

    const withPeriod = "GSTIN,Invoice No,Invoice Date,Taxable Value,Return Period\n27AABCU9603R1ZM,INV-1,04-07-2026,100000,08-2026";
    expect(normalize(withPeriod)[0]!.period).toBe("2026-08");
  });

  it("reads every row, not just the first", () => {
    const csv = `${HEADER}\n${ROW}\n27AABCU9603R1ZM,Umbrella Supplies,INV-1002,09-07-2026,95000,17100,0,0,0`;

    expect(normalize(csv).map((i) => i.invoiceNumber)).toEqual(["INV-1001", "INV-1002"]);
  });

  /** Every register export a human has touched ends in a totals row, and a
   *  blank line or two. Reading them as invoices invents purchases. */
  it("skips blank rows and a trailing total", () => {
    const csv = `${HEADER}\n${ROW}\n\n,,Total,,100000.00,18000.00,0,0,0`;

    expect(normalize(csv)).toHaveLength(1);
  });

  it("skips a row with no invoice number rather than importing a nameless invoice", () => {
    const csv = `${HEADER}\n27AABCU9603R1ZM,X,,04-07-2026,100000,18000,0,0,0`;

    expect(normalize(csv)).toEqual([]);
  });
});

describe("refusing a file rather than reporting a false reconciliation", () => {
  /** The single most important failure in this module. Matching is on GSTIN
   *  plus invoice number; a register with no GSTIN column joins to nothing, so
   *  every invoice in the authority's records reads as missing from the books
   *  and every booked invoice reads as never filed. Importing it "successfully"
   *  produces a page of confident, entirely wrong findings. */
  it("refuses a register with no GSTIN column, naming the column it wanted", () => {
    const csv = "Supplier,Invoice No,Invoice Date,Taxable Value\nUmbrella,INV-1,04-07-2026,100000";

    expect(() => normalize(csv)).toThrow(GstImportError);
    expect(() => normalize(csv)).toThrow(/GSTIN/);
  });

  it("refuses a register with no invoice number column", () => {
    const csv = "GSTIN,Supplier,Invoice Date,Taxable Value\n27AABCU9603R1ZM,Umbrella,04-07-2026,100000";

    expect(() => normalize(csv)).toThrow(/invoice number/i);
  });

  it("refuses a register with no invoice date column", () => {
    const csv = "GSTIN,Supplier,Invoice No,Taxable Value\n27AABCU9603R1ZM,Umbrella,INV-1,100000";

    expect(() => normalize(csv)).toThrow(/date/i);
  });

  it("refuses a file that is not a table at all", () => {
    expect(() => normalize("")).toThrow(GstImportError);
    expect(() => normalize("{\"b2b\": []}")).toThrow(GstImportError);
  });

  /** A date the importer cannot place must stop the import, not be guessed:
   *  the finding rules compare dates lexicographically, so a mis-read date
   *  makes a rule cite the wrong month while looking authoritative. */
  it("names the row whose date it could not place", () => {
    const csv = `${HEADER}\n27AABCU9603R1ZM,X,INV-1,fourth of July,100000,18000,0,0,0`;

    expect(() => normalize(csv)).toThrow(/row 2/);
  });
});

describe("the Turtle a pack rule cannot tell from a hand-written fixture", () => {
  it("writes the register class", () => {
    expect(toTurtle(normalize(`${HEADER}\n${ROW}`))).toContain("gst:pr-INV-1001 rdf:type gst:PurchaseInvoice");
  });

  it("makes the supplier a subject and reaches it by an edge", () => {
    const turtle = toTurtle(normalize(`${HEADER}\n${ROW}`));

    expect(turtle).toContain("gst:supplier-27AABCU9603R1ZM rdf:type gst:Supplier");
    expect(turtle).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM");

    const invoiceBlock = turtle.slice(turtle.indexOf("gst:pr-INV-1001"));
    expect(invoiceBlock).not.toContain("gst:supplierGstin");
  });

  it("writes one supplier subject however many invoices they issued", () => {
    const csv = `${HEADER}\n${ROW}\n27AABCU9603R1ZM,Umbrella Supplies,INV-1002,09-07-2026,95000,17100,0,0,0`;

    expect((toTurtle(normalize(csv)).match(/rdf:type gst:Supplier/g) ?? []).length).toBe(1);
  });

  /** The flag two of the new rules read to decide whether to fire at all. If
   *  it does not reach the graph, every reverse-charge invoice in the register
   *  is reported as one the supplier failed to file. */
  it("writes the reverse-charge flag through to the graph", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value,RCM\n27AABCU9603R1ZM,INV-1,04-07-2026,100000,Y";

    expect(toTurtle(normalize(csv))).toContain('gst:reverseCharge "Y"');
  });

  it("omits a field the register did not carry rather than writing it blank", () => {
    const csv = "GSTIN,Invoice No,Invoice Date,Taxable Value\n27AABCU9603R1ZM,INV-1,04-07-2026,100000";

    expect(toTurtle(normalize(csv))).not.toContain("gst:reverseCharge");
  });
});

describe("the canonical gst:Invoice — Plan 109 Slice 2", () => {
  it("emits a canonical subject, deterministically keyed on the GSTIN and invoice number", () => {
    const turtle = toTurtle(normalize(`${HEADER}\n${ROW}`));

    expect(turtle).toContain("gst:invoice-27AABCU9603R1ZM-INV1001 rdf:type gst:Invoice");
  });

  it("points the canonical subject at the register row with recordedIn", () => {
    const turtle = toTurtle(normalize(`${HEADER}\n${ROW}`));
    const canonicalBlock = turtle.slice(turtle.indexOf("gst:invoice-27AABCU9603R1ZM-INV1001"));

    expect(canonicalBlock).toContain("gst:recordedIn    gst:pr-INV-1001");
  });

  /** The canonical subject also carries `issuedBy` — the diagram's own top
   *  edge — as a real, unquoted reference to the supplier, not a quoted
   *  string that happens to look like one. */
  it("points the canonical subject at the supplier with an unquoted issuedBy edge", () => {
    const turtle = toTurtle(normalize(`${HEADER}\n${ROW}`));
    const canonicalStart = turtle.indexOf("gst:invoice-27AABCU9603R1ZM-INV1001");
    const canonicalBlock = turtle.slice(canonicalStart, turtle.indexOf("\n\n", canonicalStart));

    expect(canonicalBlock).toContain("gst:issuedBy      gst:supplier-27AABCU9603R1ZM ;");
    expect(canonicalBlock).not.toContain('"gst:supplier-27AABCU9603R1ZM"');
  });

  /** No Filing/Statement concept on the books side — Plan 109 decision 3. A
   *  purchase-register entry is the taxpayer's own bookkeeping, not a
   *  government submission. */
  it("emits no filedIn or appearsIn edge, only recordedIn", () => {
    const turtle = toTurtle(normalize(`${HEADER}\n${ROW}`));
    const canonicalBlock = turtle.slice(
      turtle.indexOf("gst:invoice-27AABCU9603R1ZM-INV1001"),
      turtle.indexOf("gst:invoice-27AABCU9603R1ZM-INV1001") + 200,
    );

    expect(canonicalBlock).not.toContain("gst:filedIn");
    expect(canonicalBlock).not.toContain("gst:appearsIn");
  });

  it("computes one canonical subject per invoice, not one per register row shared across invoices", () => {
    const csv = `${HEADER}\n${ROW}\n27AABCU9603R1ZM,Umbrella Supplies,INV-1002,09-07-2026,95000,17100,0,0,0`;
    const turtle = toTurtle(normalize(csv));

    expect(turtle).toContain("gst:invoice-27AABCU9603R1ZM-INV1001 rdf:type gst:Invoice");
    expect(turtle).toContain("gst:invoice-27AABCU9603R1ZM-INV1002 rdf:type gst:Invoice");
  });
});
