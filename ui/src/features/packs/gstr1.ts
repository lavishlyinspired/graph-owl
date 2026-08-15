/** GSTR-2A / GSTR-1 JSON → the GST pack's vocabulary — Plan 108 Slice 1.
 *
 *  **What a recipient can actually obtain, and why it lands as
 *  `gst:Gstr1Invoice`.** A taxpayer reconciling *purchases* cannot download
 *  their suppliers' GSTR-1 filings; what they can download is their own
 *  GSTR-2A, whose entire content *is* what those suppliers declared in
 *  GSTR-1. So this surface reads a GSTR-2A download and lands it as the
 *  declared-by-supplier class. Plan 108's vocabulary decision refuses a
 *  separate `gst:Gstr2aInvoice` for exactly this reason: 2A is a revolving
 *  view over the same supplier-declared data, and modelling both would give
 *  the graph two sources for one fact with no query needing the distinction.
 *
 *  **Field names come from a published GSP/GSTN API reference, not from
 *  memory**, and three of them are traps a guess gets wrong:
 *
 *  - `fldtr1` (filing date) and `flprdr1` (filing period) sit on the
 *    **supplier** block, not the invoice — one filing covers every invoice in
 *    it, and looking for them on `inv[]` finds nothing.
 *  - Those two use formats nothing else in the document uses: the reference's
 *    own examples are `"12-May-20"` and `"Apr-18"`, where invoice dates are
 *    `DD-MM-YYYY`.
 *  - Taxable value and tax are **per line item** under `itms[].itm_det`, and
 *    one invoice routinely carries several rate slabs. Reading the first slab
 *    under-reports every multi-rate invoice — which then surfaces as a
 *    books-vs-GSTR-1 mismatch that is really an importer bug, i.e. the worst
 *    possible failure: a false accusation about somebody's books.
 *
 *  The normalizers are shared with `gstr2b.ts` through `gstText.ts` rather
 *  than reimplemented — Plan 108's own warning, and the reason is that two
 *  `money()`s disagreeing in the last decimal produce mismatches that are not
 *  there, with nothing in the output to say which importer was wrong. */

import {
  GstImportError,
  invoiceKey,
  invoiceSubject,
  isoDate,
  money,
  monthNameDate,
  monthNamePeriod,
  returnPeriod,
  subjectSuffix,
  supplierSubject,
  turtleSubject,
} from "./gstText";

export interface Gstr1Invoice {
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
  readonly invoiceType: string;
  readonly placeOfSupply: string;
  /** When the supplier actually submitted the return. Empty when the download
   *  did not carry it — absent is a fact, and inventing today's date here
   *  would make every invoice look filed on time. */
  readonly filedDate: string;
  /** The period the supplier filed *for*, which is not the invoice's own
   *  month whenever they filed late — the case this whole plan exists for. */
  readonly period: string;
}

/** The supplier's filing date, in each of the three forms the reference
 *  emits. Absent stays absent: a missing filing date must not become today's,
 *  or every late filing reads as punctual. */
export function filedDate(value: string | undefined): string {
  const text = String(value ?? "").trim();
  if (text === "") return "";
  const named = monthNameDate(text);
  if (named !== "") return named;
  return isoDate(text);
}

/** The supplier's filing period, as the `YYYY-MM` the rules compare in. */
export function filedPeriod(value: string | undefined): string {
  const text = String(value ?? "").trim();
  if (text === "") return "";
  const named = monthNamePeriod(text);
  if (named !== "") return named;
  return returnPeriod(text);
}

/** The B2B section, wherever the wrapper puts it.
 *
 *  A GSP nests the return under `data`; a portal download does not. Finding it
 *  either way costs nothing and means the same importer accepts both without
 *  the user having to know which they have. */
function returnBody(payload: unknown): Record<string, unknown> {
  let seen: unknown = payload;
  for (let depth = 0; depth < 4; depth += 1) {
    if (typeof seen !== "object" || seen === null) break;
    const record = seen as Record<string, unknown>;
    if ("b2b" in record || "fp" in record) return record;
    seen = record.data;
  }
  throw new GstImportError(
    "this is not a GSTR-1 or GSTR-2A download — it carries no 'b2b' or 'fp' section, and " +
      "reading it as an empty return would report every booked invoice as one the supplier never filed",
  );
}

/** Every rate slab on one invoice, summed.
 *
 *  A single invoice with one line at 18% and one at 5% is ordinary. Summing is
 *  the only reading that reconciles against a purchase register, which records
 *  one figure for the invoice as a whole. */
function slabTotals(items: unknown): { taxable: number; igst: number; cgst: number; sgst: number; cess: number } {
  const totals = { taxable: 0, igst: 0, cgst: 0, sgst: 0, cess: 0 };
  for (const line of (Array.isArray(items) ? items : []) as Record<string, unknown>[]) {
    const detail = (line.itm_det ?? line) as Record<string, unknown>;
    totals.taxable += Number(money(detail.txval));
    totals.igst += Number(money(detail.iamt));
    totals.cgst += Number(money(detail.camt));
    totals.sgst += Number(money(detail.samt));
    totals.cess += Number(money(detail.csamt));
  }
  return totals;
}

export function normalize(payload: unknown): Gstr1Invoice[] {
  const body = returnBody(payload);
  const suppliers = body.b2b;
  if (suppliers === undefined) return [];
  if (!Array.isArray(suppliers)) throw new GstImportError("'b2b' is present but is not a list of suppliers");

  const invoices: Gstr1Invoice[] = [];
  for (const supplier of suppliers as Record<string, unknown>[]) {
    const gstin = String(supplier.ctin ?? "");
    const name = String(supplier.trdnm ?? "");
    // Both live on the supplier block, and both fall back to the return's own
    // `fp` when the download omits them — a GSTR-1 the supplier exported
    // themselves has no `flprdr1`, because the whole file is one period.
    const supplierFiledDate = filedDate(supplier.fldtr1 as string | undefined);
    const supplierPeriod =
      filedPeriod(supplier.flprdr1 as string | undefined) || filedPeriod(body.fp as string | undefined);

    for (const line of (supplier.inv ?? []) as Record<string, unknown>[]) {
      const slabs = slabTotals(line.itms);
      const igst = slabs.igst.toFixed(2);
      const cgst = slabs.cgst.toFixed(2);
      const sgst = slabs.sgst.toFixed(2);
      const cess = slabs.cess.toFixed(2);
      invoices.push({
        supplierGstin: gstin,
        supplierName: name,
        invoiceNumber: String(line.inum ?? ""),
        invoiceDate: isoDate(String(line.idt ?? "")),
        taxableValue: slabs.taxable.toFixed(2),
        igst,
        cgst,
        sgst,
        cess,
        // The register records one tax figure and the return splits it four
        // ways, so the total is what reconciles. The components stay as
        // evidence: an intra-state supply reported as inter-state is a real
        // and common error the total alone hides completely.
        taxAmount: (slabs.igst + slabs.cgst + slabs.sgst + slabs.cess).toFixed(2),
        reverseCharge: String(line.rchrg ?? ""),
        invoiceType: String(line.inv_typ ?? ""),
        placeOfSupply: String(line.pos ?? ""),
        filedDate: supplierFiledDate,
        period: supplierPeriod,
      });
    }
  }
  return invoices;
}

/** The same shape `packs/gst/fixtures/gstr1.ttl` carries, so the finding rules
 *  cannot tell an uploaded file from a fixture. */
export function toTurtle(invoices: readonly Gstr1Invoice[], prefix = "gst", iri?: string): string {
  const namespace = iri ?? "https://graph-owl.dev/packs/gst#";
  const lines = [
    "# GSTR-1/GSTR-2A, imported through the console. Generated — do not edit.",
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

  // **The filings — one per (supplier, return period), deduplicated across
  // every invoice line they cover — Plan 109 Slice 2.** `fldtr1`/`flprdr1`
  // live on the supplier block in the source format, not the invoice, so
  // every invoice in one group shares the identical filing date and period
  // by construction; the first one seen is as good as any other.
  const filingSubject = (gstin: string, period: string): string =>
    `${prefix}:g1filing-${subjectSuffix(gstin)}-${period}`;
  const filings = new Map<string, { readonly gstin: string; readonly period: string; readonly filedDate: string }>();
  for (const invoice of invoices) {
    if (invoice.period === "") continue;
    const key = filingSubject(invoice.supplierGstin, invoice.period);
    if (!filings.has(key)) {
      filings.set(key, { gstin: invoice.supplierGstin, period: invoice.period, filedDate: invoice.filedDate });
    }
  }
  for (const [subject, filing] of filings) {
    lines.push(
      ...turtleSubject(prefix, subject, "Gstr1Filing", [
        ["period", filing.period],
        ["filedDate", filing.filedDate],
        ["filedBy", supplierSubject(prefix, filing.gstin), true],
      ]),
    );
  }

  for (const invoice of invoices) {
    lines.push(
      ...turtleSubject(prefix, `${prefix}:g1-${subjectSuffix(invoice.invoiceNumber)}`, "Gstr1Invoice", [
        ["issuedBy", supplierSubject(prefix, invoice.supplierGstin), true],
        ["invoiceNumber", invoice.invoiceNumber],
        ["invoiceKey", invoiceKey(invoice.invoiceNumber)],
        ["invoiceDate", invoice.invoiceDate],
        ["taxableValue", invoice.taxableValue],
        ["taxAmount", invoice.taxAmount],
        ["igst", invoice.igst],
        ["cgst", invoice.cgst],
        ["sgst", invoice.sgst],
        ["cess", invoice.cess],
        ["reverseCharge", invoice.reverseCharge],
        ["invoiceType", invoice.invoiceType],
        ["placeOfSupply", invoice.placeOfSupply],
        [
          "filedIn",
          invoice.period === "" ? "" : filingSubject(invoice.supplierGstin, invoice.period),
          true,
        ],
      ]),
    );
    // The canonical entity — Plan 109 Slice 2.
    lines.push(
      ...turtleSubject(prefix, invoiceSubject(prefix, invoice.supplierGstin, invoice.invoiceNumber), "Invoice", [
        ["issuedBy", supplierSubject(prefix, invoice.supplierGstin), true],
        ["appearsIn", `${prefix}:g1-${subjectSuffix(invoice.invoiceNumber)}`, true],
      ]),
    );
  }
  return lines.join("\n");
}
