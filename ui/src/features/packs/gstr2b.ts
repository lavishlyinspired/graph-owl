/** GSTR-2B JSON → the GST pack's vocabulary, in the browser — Epic 105 P2.
 *
 *  **Why this exists in TypeScript when `connectors/python` already does it.**
 *  The path real users take is downloading a GSTR-2B JSON from the portal and
 *  uploading it; that upload happens in a browser, and there is no server route
 *  that turns a provider's JSON into RDF — `POST /graph/import/rdf` takes RDF
 *  already. So either the browser normalizes, or the console can only tell
 *  somebody to go and run a CLI, which is not an import feature.
 *
 *  **The duplication is real and is the cost of that decision.** It is bounded
 *  by being pinned to the same assertions as the Python implementation: the
 *  same fixture, the same expected Turtle. If the two drift, the tests fail.
 *  The alternative — a server route that shells out to the Python connector —
 *  would put a domain-specific normalizer inside the server, which is exactly
 *  what `plans/105-domain-neutrality.md` refuses.
 *
 *  Field names come from a published GSP API reference, not from memory:
 *  `ctin`, `trdnm`, `inum`, `dt`, `txval`, `igst`/`cgst`/`sgst`/`cess`,
 *  `itcavl`, `rev`, `typ`, `pos`, under `docdata.b2b[].inv[]`. */

/** **One error type for every GST import surface, aliased rather than
 *  subclassed.** A subclass would make `toThrow(Gstr2bError)` fail the moment
 *  a shared normalizer threw the base — the pinned tests would start failing
 *  for a reason that has nothing to do with what they assert. Callers here
 *  only ever read `.message`, which is written for whoever is uploading. */
export { GstImportError as Gstr2bError } from "./gstText";

/** **Shared with `gstr1.ts` and `books.ts`, not reimplemented beside them.**
 *  Plan 108's own warning: a second importer needs the *identical* normalizer.
 *  Two `money()`s that disagree in the last decimal place produce a
 *  reconciliation reporting mismatches that are not there, with nothing in the
 *  output to say which importer was wrong.
 *
 *  Re-exported because the pinned Python-twin tests import `isoDate` and
 *  `returnPeriod` from this module by name. */
import {
  GstImportError,
  invoiceKey,
  invoiceSubject,
  isoDate,
  literal,
  money,
  returnPeriod,
  subjectSuffix,
  supplierSubject,
} from "./gstText";

export { isoDate, returnPeriod };

export interface Gstr2bInvoice {
  readonly supplierGstin: string;
  readonly supplierName: string;
  readonly invoiceNumber: string;
  /** What the rules join on — see `invoiceKey` in `gstText.ts`. */
  readonly invoiceKey: string;
  readonly invoiceDate: string;
  readonly taxableValue: string;
  readonly igst: string;
  readonly cgst: string;
  readonly sgst: string;
  readonly cess: string;
  readonly taxAmount: string;
  readonly itcAvailable: string;
  readonly reverseCharge: string;
  readonly invoiceType: string;
  readonly placeOfSupply: string;
  readonly period: string;
}

/** The `docdata` section, wherever the wrapper puts it.
 *
 *  A GSP nests it at `data.data.docdata`; a portal download uses
 *  `data.docdata`. Finding it wherever it sits costs nothing and means the
 *  same importer accepts both without the user having to know which they have. */
function docdata(payload: unknown): Record<string, unknown> {
  let seen: unknown = payload;
  for (let depth = 0; depth < 4; depth += 1) {
    if (typeof seen !== "object" || seen === null) break;
    const record = seen as Record<string, unknown>;
    if ("docdata" in record) {
      const found = record.docdata;
      if (typeof found === "object" && found !== null) return found as Record<string, unknown>;
      break;
    }
    seen = record.data;
  }
  throw new GstImportError(
    "no 'docdata' section in this file — it is not a GSTR-2B download, and " +
      "reading it as an empty return would report every claimed invoice as unmatched",
  );
}

export function normalize(payload: unknown): Gstr2bInvoice[] {
  const suppliers = docdata(payload).b2b;
  if (suppliers === undefined) return [];
  if (!Array.isArray(suppliers)) throw new GstImportError("'b2b' is present but is not a list of suppliers");

  const invoices: Gstr2bInvoice[] = [];
  for (const supplier of suppliers as Record<string, unknown>[]) {
    const gstin = String(supplier.ctin ?? "");
    const name = String(supplier.trdnm ?? "");
    const period = returnPeriod(String(supplier.supprd ?? ""));
    for (const line of (supplier.inv ?? []) as Record<string, unknown>[]) {
      const igst = money(line.igst);
      const cgst = money(line.cgst);
      const sgst = money(line.sgst);
      const cess = money(line.cess);
      const invoiceDate = isoDate(String(line.dt ?? ""));
      invoices.push({
        supplierGstin: gstin,
        supplierName: name,
        invoiceNumber: String(line.inum ?? ""),
        invoiceKey: invoiceKey(String(line.inum ?? "")),
        invoiceDate,
        taxableValue: money(line.txval),
        igst,
        cgst,
        sgst,
        cess,
        // The register records one tax figure and the authority splits it four
        // ways, so the total is what reconciles. The components are kept as
        // evidence: an intra-state supply reported as inter-state is a real
        // and common error the total alone hides completely.
        taxAmount: (Number(igst) + Number(cgst) + Number(sgst) + Number(cess)).toFixed(2),
        itcAvailable: String(line.itcavl ?? ""),
        reverseCharge: String(line.rev ?? ""),
        invoiceType: String(line.typ ?? ""),
        placeOfSupply: String(line.pos ?? ""),
        period,
      });
    }
  }
  return invoices;
}

const LITERAL_FIELDS: readonly (readonly [string, keyof Gstr2bInvoice])[] = [
  ["invoiceNumber", "invoiceNumber"],
  ["invoiceKey", "invoiceKey"],
  ["invoiceDate", "invoiceDate"],
  ["taxableValue", "taxableValue"],
  ["taxAmount", "taxAmount"],
  ["igst", "igst"],
  ["cgst", "cgst"],
  ["sgst", "sgst"],
  ["cess", "cess"],
  ["itcAvailable", "itcAvailable"],
  ["reverseCharge", "reverseCharge"],
  ["invoiceType", "invoiceType"],
  ["placeOfSupply", "placeOfSupply"],
  // `period` moved to `gst:Gstr2bStatement` — Plan 109 Slice 2. The per-line
  // record reaches it through `reflectedIn` instead of carrying it directly.
];

/** The same shape `packs/gst/fixtures/gstr2b.ttl` carries, so the six finding
 *  rules cannot tell an uploaded file from a fixture.
 *
 *  **`gst:Supplier` is a real subject, not a literal on the invoice — Epic
 *  105c.** `plans/105c-gst-causal-graph.md` names the gap directly: the
 *  class was declared and never instantiated. One subject per unique GSTIN,
 *  deduplicated across invoices; each invoice points at it with `issuedBy`,
 *  written unquoted like `onInvoice` already is, so it resolves as an edge. */
export function toTurtle(invoices: readonly Gstr2bInvoice[], prefix = "gst", iri?: string): string {
  const namespace = iri ?? "https://graph-owl.dev/packs/gst#";
  const lines = [
    "# GSTR-2B, imported through the console. Generated — do not edit.",
    "",
    `@prefix ${prefix}: <${namespace}> .`,
    "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .",
    "",
  ];

  const supplierNames = new Map<string, string>();
  for (const invoice of invoices) {
    if (!supplierNames.has(invoice.supplierGstin)) {
      supplierNames.set(invoice.supplierGstin, invoice.supplierName);
    }
  }
  for (const [gstin, name] of supplierNames) {
    lines.push(`${supplierSubject(prefix, gstin)} rdf:type ${prefix}:Supplier ;`);
    const present = ([["supplierGstin", gstin] as const, ["supplierName", name] as const]).filter(
      ([, value]) => value !== "",
    );
    present.forEach(([fieldName, value], index) => {
      const terminator = index === present.length - 1 ? " ." : " ;";
      lines.push(`    ${prefix}:${fieldName.padEnd(13)} "${literal(value)}"${terminator}`);
    });
    lines.push("");
  }

  // **The statements — one per return period, deduplicated across every
  // invoice line they cover, generated for the single well-known Recipient
  // this pack is single-tenant for — Plan 109 Slice 2.**
  const statementSubject = (period: string): string => `${prefix}:g2bstatement-${period}`;
  const periods = new Set(invoices.map((invoice) => invoice.period).filter((period) => period !== ""));
  if (periods.size > 0) {
    lines.push(`${prefix}:recipient-self rdf:type ${prefix}:Recipient .`, "");
  }
  for (const period of periods) {
    lines.push(
      `${statementSubject(period)} rdf:type ${prefix}:Gstr2bStatement ;`,
      `    ${prefix}:${"period".padEnd(13)} "${literal(period)}" ;`,
      `    ${prefix}:${"generatedFor".padEnd(13)} ${prefix}:recipient-self .`,
      "",
    );
  }

  for (const invoice of invoices) {
    lines.push(`${prefix}:2b-${subjectSuffix(invoice.invoiceNumber)} rdf:type ${prefix}:Gstr2bInvoice ;`);
    // An absent value is omitted rather than written blank: "not reported" and
    // "reported as empty" are different facts, and a reconciliation is mostly
    // a set of questions about missing data.
    const presentLiterals = LITERAL_FIELDS.filter(([, key]) => invoice[key] !== "");
    const hasStatement = invoice.period !== "";
    const total = 1 + presentLiterals.length + (hasStatement ? 1 : 0);
    let written = 0;
    written += 1;
    lines.push(
      `    ${prefix}:${"issuedBy".padEnd(13)} ${supplierSubject(prefix, invoice.supplierGstin)}${
        written === total ? " ." : " ;"
      }`,
    );
    presentLiterals.forEach(([name, key]) => {
      written += 1;
      const terminator = written === total ? " ." : " ;";
      lines.push(`    ${prefix}:${name.padEnd(13)} "${literal(invoice[key])}"${terminator}`);
    });
    if (hasStatement) {
      lines.push(`    ${prefix}:${"reflectedIn".padEnd(13)} ${statementSubject(invoice.period)} .`);
    }
    lines.push("");

    // The canonical entity — Plan 109 Slice 2.
    const canonical = invoiceSubject(prefix, invoice.supplierGstin, invoice.invoiceNumber);
    lines.push(
      `${canonical} rdf:type ${prefix}:Invoice ;`,
      `    ${prefix}:${"issuedBy".padEnd(13)} ${supplierSubject(prefix, invoice.supplierGstin)} ;`,
      `    ${prefix}:${"reflectedIn".padEnd(13)} ${prefix}:2b-${subjectSuffix(invoice.invoiceNumber)} .`,
      "",
    );
  }
  return lines.join("\n");
}
