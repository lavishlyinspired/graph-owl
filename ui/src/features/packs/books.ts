/** The purchase register, from wherever a CA actually keeps it — Plan 108.
 *
 *  **Without this surface the product only reconciles its own demo data.**
 *  Every finding in this pack compares the taxpayer's books against what the
 *  authority holds, and until now the books could arrive only as a
 *  hand-written Turtle fixture shipped inside the pack. A real register lives
 *  in Tally, Busy, Zoho or a spreadsheet and comes out as CSV — so a CA could
 *  upload their GSTR-2B and reconcile it against somebody else's fixture.
 *
 *  **Column names are the whole problem, and they are not standardisable.**
 *  The practitioner reconciliation formats this was checked against use `GST
 *  NO.` / `PARTICULARS` / `INV. NO` / `DATE` / `BASIC`; an ERP export says
 *  `Supplier GSTIN` / `Voucher No` / `Taxable Value`. Both carry the same six
 *  facts. Matching on a normalized alias is what lets one importer read both
 *  without asking a CA to rename columns before they can use the product.
 *
 *  **Refusing beats importing a subset, and this module refuses loudly.**
 *  Matching is on GSTIN plus invoice number: a register imported without a
 *  GSTIN column joins to nothing, so every authority record reads as missing
 *  from the books and every booked invoice reads as never filed. That is not a
 *  degraded result, it is a page of confident and entirely wrong findings —
 *  so a missing required column stops the import and names itself.
 *
 *  The normalizers are shared with `gstr2b.ts` and `gstr1.ts` through
 *  `gstText.ts`. Plan 108's own warning: two `money()`s disagreeing in the
 *  last decimal produce mismatches that are not there. */

import {
  GstImportError,
  invoiceKey,
  invoiceSubject,
  isoDate,
  money,
  subjectSuffix,
  supplierSubject,
  turtleSubject,
} from "./gstText";

export interface BooksInvoice {
  readonly supplierGstin: string;
  readonly supplierName: string;
  readonly invoiceNumber: string;
  readonly invoiceDate: string;
  readonly taxableValue: string;
  readonly igst: string;
  readonly cgst: string;
  readonly sgst: string;
  readonly cess: string;
  readonly taxAmount: string;
  readonly reverseCharge: string;
  readonly period: string;
}

/** Rows out of a CSV or TSV export.
 *
 *  Written rather than taken from a library on purpose: the whole requirement
 *  is quoted fields, doubled quotes, CRLF and a tab option, which is thirty
 *  lines — and this file runs in the browser, where a parser dependency is
 *  bytes on every page load for one upload surface. `plans/00l-build-vs-adopt.md`
 *  records the check.
 *
 *  **The delimiter is sniffed from the header**, because "Save as Unicode
 *  text" in a spreadsheet produces TSV and is exactly what a user reaches for
 *  when a CSV mangled their data. */
export function parseDelimited(text: string): string[][] {
  const source = text.replace(/^\uFEFF/, "");
  const firstLine = source.slice(0, source.indexOf("\n") === -1 ? undefined : source.indexOf("\n"));
  const delimiter = firstLine.includes("\t") && !firstLine.includes(",") ? "\t" : ",";

  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;

  const endField = () => {
    row.push(field.trim());
    field = "";
  };
  const endRow = () => {
    endField();
    // A line that is entirely empty is a spreadsheet artefact, not a record.
    if (row.some((cell) => cell !== "")) rows.push(row);
    row = [];
  };

  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    if (quoted) {
      if (char === '"') {
        if (source[index + 1] === '"') {
          field += '"';
          index += 1;
        } else {
          quoted = false;
        }
      } else {
        field += char;
      }
      continue;
    }
    if (char === '"') quoted = true;
    else if (char === delimiter) endField();
    else if (char === "\n") endRow();
    else if (char !== "\r") field += char;
  }
  if (field !== "" || row.length > 0) endRow();
  return rows;
}

/** A header cell reduced to what is actually distinctive about it, so
 *  `GST NO.`, `gstin`, `GSTIN of Supplier` and `supplier_gstin` all land on
 *  the same key. */
function headerKey(cell: string): string {
  return cell.toLowerCase().replace(/[^a-z0-9]/g, "");
}

/** What each field may be called in the wild.
 *
 *  Longest-first within a field is deliberate: `taxamount` must not be matched
 *  by the looser `amount`, and `invoicedate` must win over a bare `date` on an
 *  export that carries both an invoice date and a posting date. */
const ALIASES: Record<keyof Omit<BooksInvoice, "taxAmount" | "period">, readonly string[]> = {
  supplierGstin: ["suppliergstin", "gstinofsupplier", "partygstin", "gstno", "gstinno", "gstin", "gstnumber"],
  supplierName: ["suppliername", "nameofsupplier", "partyname", "particulars", "supplier", "party", "tradename", "name"],
  invoiceNumber: ["invoiceno", "invoicenumber", "invno", "billno", "voucherno", "documentno", "docno"],
  invoiceDate: ["invoicedate", "invdate", "billdate", "voucherdate", "documentdate", "date"],
  taxableValue: ["taxablevalue", "taxableamount", "assessablevalue", "netvalue", "basicvalue", "basic", "taxable", "value"],
  igst: ["igstamount", "igst", "integratedtax"],
  cgst: ["cgstamount", "cgst", "centraltax"],
  sgst: ["sgstamount", "sgst", "utgst", "statetax"],
  cess: ["cessamount", "cess"],
  reverseCharge: ["reversecharge", "reversechargeapplicable", "rcm", "revcharge", "rchrg"],
};

const TAX_TOTAL_ALIASES = ["taxamount", "totaltax", "gstamount", "totalgst", "taxtotal"];
const PERIOD_ALIASES = ["returnperiod", "taxperiod", "period", "month"];

/** Where each field was found, or -1.
 *
 *  **A named shape rather than a `Record<string, number>`**: with an index
 *  signature every lookup is `number | undefined`, so the one thing this type
 *  exists to guarantee — that every field was looked for — is exactly what it
 *  stops expressing. Adding a field to {@link BooksInvoice} without teaching
 *  `columns` about it should be a compile error, and here it is one. */
type ColumnIndex = Record<keyof typeof ALIASES, number> & { taxAmount: number; period: number };

/** First alias match wins, so a duplicated header does not silently shadow the
 *  one the export meant. */
function columns(header: readonly string[]): ColumnIndex {
  const keys = header.map(headerKey);
  const locate = (aliases: readonly string[]): number => {
    for (const alias of aliases) {
      const index = keys.indexOf(alias);
      if (index !== -1) return index;
    }
    return -1;
  };
  return {
    supplierGstin: locate(ALIASES.supplierGstin),
    supplierName: locate(ALIASES.supplierName),
    invoiceNumber: locate(ALIASES.invoiceNumber),
    invoiceDate: locate(ALIASES.invoiceDate),
    taxableValue: locate(ALIASES.taxableValue),
    igst: locate(ALIASES.igst),
    cgst: locate(ALIASES.cgst),
    sgst: locate(ALIASES.sgst),
    cess: locate(ALIASES.cess),
    reverseCharge: locate(ALIASES.reverseCharge),
    taxAmount: locate(TAX_TOTAL_ALIASES),
    period: locate(PERIOD_ALIASES),
  };
}

function cell(row: readonly string[], index: number): string {
  return index === -1 ? "" : (row[index] ?? "").trim();
}

/** `Y`/`N`, whatever the export wrote.
 *
 *  The rules compare against the literal `"Y"` the GST return format uses, so
 *  a register saying `Yes` or `TRUE` must be normalized or two rules will fire
 *  on every reverse-charge invoice it holds. */
function reverseChargeFlag(value: string): string {
  const text = value.trim().toLowerCase();
  if (text === "") return "";
  return ["y", "yes", "true", "1", "r"].includes(text) ? "Y" : "N";
}

/** `YYYY-MM` from whatever a register calls a period — `MM-YYYY`, `MMYYYY`,
 *  `YYYY-MM` or a full date. Anything else is ignored rather than refused: the
 *  invoice date is a correct fallback, so an unreadable period column is not
 *  worth failing an import over. */
function periodOf(value: string, invoiceDate: string): string {
  const text = value.trim();
  const iso = text.match(/^(\d{4})-(0[1-9]|1[0-2])/);
  if (iso) return `${iso[1]}-${iso[2]}`;
  const monthFirst = text.match(/^(0[1-9]|1[0-2])[-/]?(\d{4})$/);
  if (monthFirst) return `${monthFirst[2]}-${monthFirst[1]}`;
  return invoiceDate.slice(0, 7);
}

export function normalize(text: string): BooksInvoice[] {
  const rows = parseDelimited(text);
  if (rows.length === 0) {
    throw new GstImportError(
      "that file has no rows — a purchase register should be a CSV or tab-separated export with a header row",
    );
  }

  const header = rows[0] ?? [];
  const body = rows.slice(1);
  const at = columns(header);

  // **Named individually, not as one "missing columns" list.** A CA fixing an
  // export needs to know which column to add, and the first thing they will do
  // is look at their file — so the message has to be specific enough to act on
  // without reading any documentation.
  if (at.supplierGstin === -1) {
    throw new GstImportError(
      "no GSTIN column found in that file. Reconciliation matches on the supplier's GSTIN together " +
        "with the invoice number, so without it every invoice would be reported as unmatched. " +
        `Add a column named GSTIN (or "GST No.", "Supplier GSTIN"). Columns found: ${header.join(", ")}`,
    );
  }
  if (at.invoiceNumber === -1) {
    throw new GstImportError(
      "no invoice number column found in that file. " +
        `Add one named "Invoice No" (or "Bill No", "Voucher No"). Columns found: ${header.join(", ")}`,
    );
  }
  if (at.invoiceDate === -1) {
    throw new GstImportError(
      "no invoice date column found in that file. The date decides which provision a finding is " +
        `judged under, so it cannot be inferred. Add one named "Invoice Date". Columns found: ${header.join(", ")}`,
    );
  }

  const invoices: BooksInvoice[] = [];
  body.forEach((row, index) => {
    const invoiceNumber = cell(row, at.invoiceNumber);
    const gstin = cell(row, at.supplierGstin);
    // A totals row has figures and no invoice number; a stray fragment has
    // neither it nor a GSTIN. Neither is a purchase, and importing one invents
    // an invoice nobody issued.
    if (invoiceNumber === "" || gstin === "") return;

    let invoiceDate: string;
    try {
      invoiceDate = isoDate(cell(row, at.invoiceDate));
    } catch (error) {
      // **The row number is the file's, counting the header as row 1** — which
      // is what a CA sees when they open it in a spreadsheet to fix it. An
      // index into the data rows would be off by one from every tool they have,
      // and "not a date this importer can place" on a 900-row register is
      // unactionable without a row they can navigate to.
      throw new GstImportError(
        `row ${index + 2} (invoice ${invoiceNumber}): ${error instanceof Error ? error.message : String(error)}`,
      );
    }

    const igst = money(cell(row, at.igst));
    const cgst = money(cell(row, at.cgst));
    const sgst = money(cell(row, at.sgst));
    const cess = money(cell(row, at.cess));
    const headwise = Number(igst) + Number(cgst) + Number(sgst) + Number(cess);
    const stated = cell(row, at.taxAmount);

    invoices.push({
      supplierGstin: gstin,
      supplierName: cell(row, at.supplierName),
      invoiceNumber,
      invoiceDate,
      taxableValue: money(cell(row, at.taxableValue)),
      igst,
      cgst,
      sgst,
      cess,
      // The head-wise split is preferred when present, because it is the more
      // specific fact and it is what an intra-state-reported-as-inter-state
      // error shows up in. An export with only a total is still reconcilable
      // on the total, which is what the rules compare.
      taxAmount: headwise > 0 || stated === "" ? headwise.toFixed(2) : money(stated),
      reverseCharge: reverseChargeFlag(cell(row, at.reverseCharge)),
      period: periodOf(cell(row, at.period), invoiceDate),
    });
  });
  return invoices;
}

/** The same shape `packs/gst/fixtures/purchase-register.ttl` carries, so the
 *  finding rules cannot tell an uploaded register from the demo fixture. */
export function toTurtle(invoices: readonly BooksInvoice[], prefix = "gst", iri?: string): string {
  const namespace = iri ?? "https://graph-owl.dev/packs/gst#";
  const lines = [
    "# Purchase register, imported through the console. Generated — do not edit.",
    "",
    `@prefix ${prefix}: <${namespace}> .`,
    "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .",
    "",
  ];

  const supplierNames = new Map<string, string>();
  for (const invoice of invoices) {
    if (!supplierNames.has(invoice.supplierGstin)) supplierNames.set(invoice.supplierGstin, invoice.supplierName);
  }
  for (const [gstin, name] of supplierNames) {
    lines.push(
      ...turtleSubject(prefix, supplierSubject(prefix, gstin), "Supplier", [
        ["supplierGstin", gstin],
        ["supplierName", name],
      ]),
    );
  }

  for (const invoice of invoices) {
    lines.push(
      ...turtleSubject(prefix, `${prefix}:pr-${subjectSuffix(invoice.invoiceNumber)}`, "PurchaseInvoice", [
        ["issuedBy", supplierSubject(prefix, invoice.supplierGstin), true],
        ["invoiceNumber", invoice.invoiceNumber],
        // What the rules join on — see `invoiceKey`.
        ["invoiceKey", invoiceKey(invoice.invoiceNumber)],
        ["invoiceDate", invoice.invoiceDate],
        ["taxableValue", invoice.taxableValue],
        ["taxAmount", invoice.taxAmount],
        ["igst", invoice.igst],
        ["cgst", invoice.cgst],
        ["sgst", invoice.sgst],
        ["cess", invoice.cess],
        ["reverseCharge", invoice.reverseCharge],
        ["period", invoice.period],
      ]),
    );
    // The canonical entity — Plan 109 Slice 2. `recordedIn` only: books
    // carries no Filing/Statement concept, decision 3.
    lines.push(
      ...turtleSubject(
        prefix,
        invoiceSubject(prefix, invoice.supplierGstin, invoice.invoiceNumber),
        "Invoice",
        [
          ["issuedBy", supplierSubject(prefix, invoice.supplierGstin), true],
          ["recordedIn", `${prefix}:pr-${subjectSuffix(invoice.invoiceNumber)}`, true],
        ],
      ),
    );
  }
  return lines.join("\n");
}
