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

export class Gstr2bError extends Error {}

export interface Gstr2bInvoice {
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
  readonly itcAvailable: string;
  readonly reverseCharge: string;
  readonly invoiceType: string;
  readonly placeOfSupply: string;
  readonly period: string;
}

/** A monetary figure as a fixed two-decimal string.
 *
 *  `String(40000)` is `"40000"`, which does not equal a purchase register's
 *  `"40000.00"` — and the reconciliation compares them as strings, so a number
 *  that looks equal to a human simply fails to match. */
function money(value: unknown): string {
  if (value === null || value === undefined || value === "") return "0.00";
  const parsed = typeof value === "number" ? value : Number(String(value));
  if (!Number.isFinite(parsed)) throw new Gstr2bError(`'${String(value)}' is not a monetary amount`);
  return parsed.toFixed(2);
}

/** A GST date as ISO-8601.
 *
 *  **The trap this normalizer exists to close.** GST reports `DD-MM-YYYY`, and
 *  every finding rule compares dates as ISO strings, relying on lexicographic
 *  order being chronological order — the only reason "which provision was in
 *  force" is answerable, since the query engine has no date type. Passing
 *  `04-07-2026` through breaks that ordering *silently*: the string sorts by
 *  day-of-month, the Rule 36(4) resolution picks the wrong provision, and the
 *  finding cites the wrong notification while looking entirely authoritative.
 *
 *  ISO input passes through rather than being re-parsed day-first, and
 *  anything unplaceable is refused — a wrong citation is worse than a missing
 *  finding. */
export function isoDate(value: string): string {
  const text = String(value ?? "").trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(text)) return text;
  const dmy = text.match(/^(\d{2})[-/](\d{2})[-/](\d{4})$/);
  if (dmy) return `${dmy[3]}-${dmy[2]}-${dmy[1]}`;
  throw new Gstr2bError(
    `'${text}' is not a date this importer can place — GST reports DD-MM-YYYY ` +
      `and the rules need ISO-8601; guessing would produce a finding citing the wrong provision`,
  );
}

/** A supplier's declared return period, `MMYYYY`, as the `YYYY-MM` the rules
 *  compare periods in.
 *
 *  **This, not the invoice's own date, is what `gst:period` means.** GSTR-2B
 *  is a monthly snapshot of *filed* returns, not of invoice dates — an
 *  invoice dated in July can legitimately surface only in August's 2B, once
 *  the supplier files late. Deriving the period from `invoiceDate` instead
 *  silently makes that case, and the carry-forward reasoning built on it,
 *  untestable: every invoice would report the period it was *issued* in,
 *  never the period it was *filed* in. */
export function returnPeriod(supprd: string): string {
  const match = /^(0[1-9]|1[0-2])(\d{4})$/.exec(String(supprd ?? ""));
  if (!match) {
    throw new Gstr2bError(`'${supprd}' is not a return period this importer can place — expected 'supprd' as MMYYYY`);
  }
  const [, month, year] = match;
  return `${year}-${month}`;
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
  throw new Gstr2bError(
    "no 'docdata' section in this file — it is not a GSTR-2B download, and " +
      "reading it as an empty return would report every claimed invoice as unmatched",
  );
}

export function normalize(payload: unknown): Gstr2bInvoice[] {
  const suppliers = docdata(payload).b2b;
  if (suppliers === undefined) return [];
  if (!Array.isArray(suppliers)) throw new Gstr2bError("'b2b' is present but is not a list of suppliers");

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

/** Backslash first, or the escapes escape each other and one badly-named
 *  supplier corrupts every triple after it. */
function literal(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
}

const LITERAL_FIELDS: readonly (readonly [string, keyof Gstr2bInvoice])[] = [
  ["invoiceNumber", "invoiceNumber"],
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
  ["period", "period"],
];

/** The subject an invoice was `issuedBy`, keyed on the GSTIN so two invoices
 *  from the same supplier resolve to the same node — Epic 105c. */
function supplierSubject(prefix: string, gstin: string): string {
  return `${prefix}:supplier-${gstin}`;
}

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

  for (const invoice of invoices) {
    lines.push(`${prefix}:2b-${invoice.invoiceNumber} rdf:type ${prefix}:Gstr2bInvoice ;`);
    // An absent value is omitted rather than written blank: "not reported" and
    // "reported as empty" are different facts, and a reconciliation is mostly
    // a set of questions about missing data.
    const presentLiterals = LITERAL_FIELDS.filter(([, key]) => invoice[key] !== "");
    const total = 1 + presentLiterals.length;
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
    lines.push("");
  }
  return lines.join("\n");
}
