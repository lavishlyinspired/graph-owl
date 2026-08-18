/** The normalizers every GST import surface shares — Plan 108 Slice 1.
 *
 *  **Extracted rather than copied, and that is the point.** Plan 108's own
 *  warnings say it directly: a second importer needs the *identical*
 *  normalizer, not a rewritten one that could drift from it. Amounts that
 *  compare as strings and dates that compare lexicographically are not
 *  formatting choices here — they are what the finding rules rest on. Two
 *  implementations of `money` that disagree in the last decimal place produce
 *  a reconciliation that reports mismatches which are not there, and there is
 *  nothing in the output to say which importer was wrong.
 *
 *  `gstr2b.ts` re-exports its error type from here so the pinned
 *  TypeScript/Python fixture pair keeps asserting exactly what it did. */

/** One error for every GST import surface — a file this console could not
 *  read, with a message written for whoever is uploading it rather than for
 *  whoever wrote the parser. */
export class GstImportError extends Error {}

/** A monetary figure as a fixed two-decimal string.
 *
 *  `String(40000)` is `"40000"`, which does not equal a purchase register's
 *  `"40000.00"` — and the reconciliation compares them as strings, so a number
 *  that looks equal to a human simply fails to match. */
export function money(value: unknown): string {
  if (value === null || value === undefined || value === "") return "0.00";
  const text = typeof value === "number" ? String(value) : String(value).replace(/[\s,₹]/g, "");
  const parsed = Number(text);
  if (!Number.isFinite(parsed)) throw new GstImportError(`'${String(value)}' is not a monetary amount`);
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
  throw new GstImportError(
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
    throw new GstImportError(`'${supprd}' is not a return period this importer can place — expected 'supprd' as MMYYYY`);
  }
  const [, month, year] = match;
  return `${year}-${month}`;
}

const MONTHS: Record<string, string> = {
  jan: "01", feb: "02", mar: "03", apr: "04", may: "05", jun: "06",
  jul: "07", aug: "08", sep: "09", oct: "10", nov: "11", dec: "12",
};

/** A two- or four-digit year as four digits.
 *
 *  **Two digits are always 20xx, and that is derivable rather than assumed:**
 *  GST returns did not exist before July 2017, so no date in any of these
 *  documents can fall in the 1900s. The alternative — a sliding pivot — would
 *  make the same file parse differently in different years. */
function fourDigitYear(text: string): string {
  return text.length === 4 ? text : `20${text}`;
}

/** The `DD-Mon-YY` form a GSTR-2A download uses for the supplier's filing
 *  date, alongside the `DD-MM-YYYY` and ISO forms {@link isoDate} handles.
 *
 *  **Confined to this one field on purpose.** The published reference's own
 *  example is `"12-May-20"`, and only the supplier-level filing fields use it
 *  — invoice dates in the same document are `DD-MM-YYYY`. Teaching the shared
 *  {@link isoDate} to accept month names would widen what the GSTR-2B
 *  importer accepts too, and that importer is pinned assertion-for-assertion
 *  against its Python twin; widening one side of a pinned pair is exactly the
 *  drift the pinning exists to prevent. */
export function monthNameDate(value: string): string {
  const text = String(value ?? "").trim();
  const match = text.match(/^(\d{1,2})[-/\s]([A-Za-z]{3,})[-/\s](\d{2}|\d{4})$/);
  if (!match) return "";
  const [, day = "", name = "", year = ""] = match;
  const month = MONTHS[name.slice(0, 3).toLowerCase()];
  if (!month) return "";
  return `${fourDigitYear(year)}-${month}-${day.padStart(2, "0")}`;
}

/** The `Mon-YY` form a GSTR-2A download uses for the supplier's filing
 *  period, alongside `MMYYYY`. Same confinement, same reason. */
export function monthNamePeriod(value: string): string {
  const text = String(value ?? "").trim();
  const match = text.match(/^([A-Za-z]{3,})[-/\s](\d{2}|\d{4})$/);
  if (!match) return "";
  const [, name = "", year = ""] = match;
  const month = MONTHS[name.slice(0, 3).toLowerCase()];
  if (!month) return "";
  return `${fourDigitYear(year)}-${month}`;
}

/** Backslash first, or the escapes escape each other and one badly-named
 *  supplier corrupts every triple after it. */
export function literal(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
}

/** The key two records are the same invoice by.
 *
 *  **The single most common real-world matching failure this closes.** A
 *  supplier writes `INV-2023/01`, the taxpayer's clerk keys `INV202301`, and an
 *  exact join sees two unrelated invoices — so the register row is reported as
 *  one the supplier never filed *and* the GSTR-2B row as one never booked. Two
 *  false findings, opposite in direction, about a single invoice that matched.
 *
 *  **Written into the graph at import rather than computed in the query.**
 *  SPARQL can do this with `REPLACE`, but only inside a `FILTER` — which turns
 *  an indexable join into a cross product filtered afterwards, O(n×m) on a real
 *  register. As a stored property it stays a plain join, and it is visible to a
 *  reviewer reading the data rather than buried in five query files.
 *
 *  **Deliberately conservative: case and punctuation only.** Collapsing harder
 *  is worse than not collapsing — a missed match leaves a finding for a human,
 *  a wrong match silently claims credit against the wrong invoice. Leading
 *  zeros in particular are *not* stripped: `INV-001` and `INV-1` are different
 *  invoices in plenty of numbering schemes.
 *
 *  This is the same normalization this project's own `[[matching.blocking]]`
 *  `normalized` strategy describes — a strategy the finding rules never
 *  invoked, because they join on the raw literal. */
export function invoiceKey(value: string): string {
  return String(value ?? "").toUpperCase().replace(/[^A-Z0-9]/g, "");
}

/** An identifier as the local part of a prefixed name.
 *
 *  **The bug a real upload found, and it is not an edge case.** Indian invoice
 *  numbers routinely look like `RST/2026/0455` — a slash is the ordinary
 *  separator. Written straight into a prefixed name that is
 *  `gst:pr-RST/2026/0455`, which is not legal Turtle, and the server rejected
 *  the entire import with "1 field failed validation". Every importer here had
 *  it, including the GSTR-2B one that shipped long before this and would have
 *  failed identically on any real return whose supplier numbers that way.
 *
 *  **Percent-encoded rather than substituted, because a collision here merges
 *  two invoices.** Mapping every unsafe character onto `-` would make `INV/1`
 *  and `INV-1` one subject: two different invoices silently becoming one in a
 *  tax reconciliation, which is worse than the rejected import this replaces.
 *  Percent-encoding is reversible and is explicitly legal in a Turtle
 *  `PN_LOCAL` (the `PLX`/`PERCENT` production), so no escaping scheme of our
 *  own is needed.
 *
 *  The true value always travels beside it as a `gst:invoiceNumber` literal,
 *  and every rule joins on *that*, so the subject is free to be an identifier
 *  rather than a display value. */
export function subjectSuffix(value: string): string {
  return value.replace(/[^A-Za-z0-9_-]/g, (char) =>
    char
      .split("")
      .map((c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`)
      .join(""),
  );
}

/** The subject an invoice was `issuedBy`, keyed on the GSTIN so two invoices
 *  from the same supplier resolve to the same node — Epic 105c. */
export function supplierSubject(prefix: string, gstin: string): string {
  return `${prefix}:supplier-${subjectSuffix(gstin)}`;
}

/** The canonical `gst:Invoice` subject — Plan 109 Slice 2.
 *
 *  **Deterministic, so every importer that sees the same real invoice
 *  computes the identical subject with no explicit merge step.** Keyed on
 *  the *exact* GSTIN and the normalized `invoiceKey`, not the printed
 *  invoice number — three printed spellings of one invoice must land on one
 *  canonical subject, the same reason `invoiceKey` exists at all. A hard-key
 *  mismatch (a transposed GSTIN, a different state registration of the same
 *  PAN) computes a genuinely *different* subject, which is deliberate: this
 *  function is the exact-match path only, and per
 *  `plans/109-gst-canonical-invoice-model.md` decision 6 a hard-key
 *  collision must be reviewed, never silently merged. */
export function invoiceSubject(prefix: string, gstin: string, invoiceNumber: string): string {
  return `${prefix}:invoice-${subjectSuffix(gstin)}-${invoiceKey(invoiceNumber)}`;
}

/** One subject's predicate block, with the terminator each line needs.
 *
 *  **An absent value is omitted rather than written blank.** "Not reported"
 *  and "reported as empty" are different facts, and a reconciliation is
 *  mostly a set of questions about missing data — a blank literal would
 *  answer a question nobody asked and make an absence unfindable. */
export function turtleSubject(
  prefix: string,
  subject: string,
  type: string,
  fields: readonly (readonly [name: string, value: string, isIri?: boolean])[],
): string[] {
  const present = fields.filter(([, value]) => value !== "");
  if (present.length === 0) return [];
  const lines = [`${subject} rdf:type ${prefix}:${type} ;`];
  present.forEach(([name, value, isIri], index) => {
    const terminator = index === present.length - 1 ? " ." : " ;";
    const object = isIri ? value : `"${literal(value)}"`;
    // `padEnd(13)` is what `gstr2b.ts` and every hand-written fixture in
    // `packs/gst/fixtures/` already align on. Matching it is not cosmetic:
    // the pack's own fixtures and an uploaded file should be indistinguishable
    // in a diff, which is how a reviewer checks an importer at all.
    lines.push(`    ${prefix}:${name.padEnd(13)} ${object}${terminator}`);
  });
  lines.push("");
  return lines;
}
