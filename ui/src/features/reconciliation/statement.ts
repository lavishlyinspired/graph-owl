/** The reconciliation statement, as a CA reads one — Plan 108 Slice 7.
 *
 *  **Why this file is allowed to know what GST is.** `packSurfaces.ts` already
 *  establishes the pattern and states its own rule: per-pack console content
 *  lives in one registry object, and the components stay generic. This is the
 *  second entry in that pattern — the *shape* of the statement (an opening
 *  balance, signed reconciling items, a closing balance, and a residual) is
 *  domain-neutral and computed here for any pack's findings; only
 *  {@link SCENARIOS} names GST's rules.
 *
 *  The eventual home for {@link SCENARIOS} is `pack.toml` beside the
 *  `[[findings]]` entries it describes, surfaced through
 *  `GET /packs/{pack}/finding-rules`. That is deliberately not done yet: it is
 *  a schema change to the finding-rule record for a generalisation no second
 *  domain has asked for, and `plans/105g` set the precedent — build the narrow
 *  thing, generalise when a real second case appears rather than on the
 *  suspicion that one might.
 *
 *  **The arithmetic is the risk, not the rendering.** A wrong figure in a tax
 *  working paper is worse than no figure, because it gets signed off. So every
 *  amount here is summed from the graph's own values, and anything the rules
 *  cannot account for is reported as a residual rather than absorbed into a
 *  bucket. A statement that always balances means nothing. */

import type { PackFinding } from "../../api";

/** One invoice as it exists in one source, read straight off the graph.
 *
 *  **Amounts stay strings until they are summed**, matching how the pack
 *  stores them: a monetary value parsed into a float at the graph boundary
 *  loses the exactness a tax figure needs. */
export interface SourceInvoice {
  readonly invoiceNumber: string;
  readonly gstin: string;
  readonly supplierName: string;
  readonly invoiceDate: string;
  readonly taxableValue: string;
  readonly taxAmount: string;
  readonly period: string;
}

export interface SourceSummary {
  readonly count: number;
  readonly taxableValue: number;
  readonly taxAmount: number;
  readonly periods: readonly string[];
}

/** One line of the statement: every finding of one kind, valued and signed. */
export interface ReconcilingItem {
  readonly label: string;
  readonly title: string;
  /** `+1` raises the books above the authority, `-1` lowers it, `0` explains
   *  none of the difference (an invoice both sides agree on that is still not
   *  claimable). */
  readonly sign: number;
  readonly count: number;
  readonly taxableValue: number;
  readonly taxAmount: number;
  readonly invoices: readonly string[];
  readonly rows: readonly SourceInvoice[];
}

export interface Statement {
  readonly books: SourceSummary;
  readonly authority: SourceSummary;
  readonly difference: { readonly taxableValue: number; readonly taxAmount: number };
  readonly explained: { readonly taxableValue: number; readonly taxAmount: number };
  readonly unexplained: { readonly taxableValue: number; readonly taxAmount: number };
  readonly items: readonly ReconcilingItem[];
  /** Whether the rules account for the whole difference. */
  readonly reconciled: boolean;
}

/** What a rule means to the person who has to act on it.
 *
 *  `summary` on the rule itself is written for a reviewer of the *rule* — "an
 *  invoice claimed in the purchase register that the supplier has not reported
 *  in any GSTR-1/IFF filing". A CA looking at a queue of two hundred needs the
 *  next action, and that is a different sentence. */
export interface Scenario {
  readonly title: string;
  readonly meaning: string;
  readonly nextAction: string;
  readonly sign: number;
  readonly tone: "warning" | "danger" | "info";
}

const SCENARIOS: Record<string, Scenario> = {
  "gst:SupplierNotFiled": {
    title: "Supplier has not filed",
    meaning: "You have booked this purchase and no supplier has declared it in any GSTR-1 or IFF.",
    nextAction: "Chase the supplier to file it. Do not claim the credit until it appears in a GSTR-2B.",
    sign: 1,
    tone: "danger",
  },
  "gst:Gstr1NotIn2b": {
    title: "Filed by the supplier, not in your GSTR-2B",
    meaning:
      "The supplier did declare it — the filing date is on the finding — but it has reached none of the GSTR-2B statements loaded here.",
    nextAction:
      "Check the GSTIN they filed against and whether it went in as B2C. If they simply filed late, it should appear in a later period's 2B.",
    sign: 1,
    tone: "warning",
  },
  "gst:MissingInBooks": {
    title: "In the GST records, not in your books",
    meaning: "A supplier has declared an invoice against your GSTIN that your purchase register does not carry.",
    nextAction:
      "Find out whether the purchase was simply never recorded — this is credit you may be entitled to and have not claimed — or whether somebody has filed against the wrong GSTIN.",
    sign: -1,
    tone: "info",
  },
  "gst:BooksGstr1Mismatch": {
    title: "Your books and the supplier's filing disagree",
    meaning: "Both sides declare this invoice and the values differ by more than the cap in force allowed.",
    nextAction: "Compare against the physical invoice. One of the two is wrong, and the finding names both figures.",
    sign: 0,
    tone: "warning",
  },
  "gst:GoodsReceiptTiming": {
    title: "Goods received after the period",
    meaning:
      "Every document agrees and the credit is still not claimable in this period, because the goods or services arrived in a later one.",
    nextAction: "Defer the claim to the period the goods were received in. Section 16(2)(b) is a condition in its own right.",
    sign: 0,
    tone: "warning",
  },
  "gst:PotentialMismatch": {
    title: "Claimed, not available in GSTR-2B",
    meaning:
      "This invoice is in your books and in no GSTR-2B. No GSTR-1/2A data is loaded, so nothing here can say whether the supplier filed it.",
    nextAction: "Upload the GSTR-2A for this period — it separates 'the supplier never filed' from 'they filed and it did not reach you'.",
    sign: 1,
    tone: "danger",
  },
  "gst:AmountMismatch": {
    title: "Your books and GSTR-2B disagree",
    meaning: "Both sides report the invoice and the values differ by more than the cap in force allowed.",
    nextAction: "Compare against the physical invoice, then correct whichever side is wrong.",
    sign: 0,
    tone: "warning",
  },
  "gst:ITCNotAvailable": {
    title: "Credit reported as unavailable",
    meaning: "The invoice matches perfectly and the authority reports the credit as not available.",
    nextAction: "Check whether it falls under Section 17(5) blocked credits. Do not claim it on the strength of the match alone.",
    sign: 0,
    tone: "danger",
  },
  "gst:Reversed": {
    title: "Flagged as reverse charge",
    meaning: "The invoice matches and the authority flags it as reverse-charge, so the tax is yours to self-assess.",
    nextAction: "Confirm the tax has been paid under RCM before taking the credit.",
    sign: 0,
    tone: "info",
  },
  "gst:GstinTransposition": {
    title: "Near-identical GSTIN — probably a keying error",
    meaning:
      "Two records agree on invoice number and period under GSTINs that differ by what looks like a transposition. Nothing has been merged.",
    nextAction: "Confirm which GSTIN is right and correct the register. Merging automatically would attribute one supplier's invoice to another.",
    sign: 0,
    tone: "warning",
  },
  "gst:PaymentOverdue": {
    title: "Unpaid past 180 days",
    meaning: "Credit was taken on an invoice the supplier has not been paid for within 180 days of its date.",
    nextAction: "Reverse the credit, or pay the supplier. Section 16(2)(d) leaves no third option.",
    sign: 0,
    tone: "danger",
  },
};

/** The local name of a term, for a reader who does not want to read IRIs.
 *
 *  **Both a full IRI and a curie, because both really arrive.** A finding's
 *  `predicate` comes back expanded (`https://…/gst#taxAmount`) while its
 *  `label` stays as `pack.toml` wrote it (`gst:AmountMismatch`). Handling only
 *  the first leaves every label unmatched in {@link SCENARIOS}, so every
 *  finding renders under its raw label with no next action — the page still
 *  loads, which is exactly why it would not be noticed.
 *
 *  The `:` cut is tried last, never first: an IRI's own scheme separator is a
 *  colon, and cutting there would turn every IRI into `//graph-owl.dev/…`. */
function localName(term: string): string {
  const slash = Math.max(term.lastIndexOf("#"), term.lastIndexOf("/"));
  const cut = slash >= 0 ? slash : term.lastIndexOf(":");
  const tail = cut >= 0 ? term.slice(cut + 1) : term;
  return tail.length > 0 ? tail : term;
}

export function scenarioFor(label: string): Scenario {
  const known = SCENARIOS[label];
  if (known) return known;
  // A pack this console has never seen must still render: the label beats an
  // empty card, and beats a crash by a great deal more.
  const name = localName(label);
  return { title: name, meaning: "", nextAction: `Review ${name}.`, sign: 0, tone: "info" };
}

/** A finding's evidence, indexed by the predicate each fact came from.
 *
 *  **Values are a list, not a value.** Two facts under one predicate is the
 *  normal case: `AmountMismatch` projects the register's `taxableValue` and
 *  the authority's, both as `gst:taxableValue`, because that pair *is* the
 *  finding. Keeping only one would render a mismatch with a single number. */
export type EvidenceIndex = Readonly<Record<string, readonly string[] | undefined>>;

export function evidenceOf(finding: PackFinding): EvidenceIndex {
  const found: Record<string, string[]> = {};
  for (const fact of finding.evidence) {
    const key = localName(fact.predicate);
    (found[key] ??= []).push(fact.value);
  }
  return found;
}

/** Every value a rule projected under one predicate, or none.
 *
 *  **A function rather than a defaulting Proxy on the index.** The Proxy this
 *  replaced made every lookup *look* total to a reader while the type stayed
 *  `string[] | undefined` — so the code passed review and failed the compiler,
 *  and the only way to make it pass would have been to assert away the exact
 *  guarantee that was in doubt. A plain accessor is honest in both directions. */
export function values(index: EvidenceIndex, predicate: string): readonly string[] {
  return index[predicate] ?? [];
}

/** The first value under a predicate, or `""` — for the many places that want
 *  a date or a citation to render and have nothing sensible to do with a list. */
export function first(index: EvidenceIndex, predicate: string): string {
  return values(index, predicate)[0] ?? "";
}

/** Which invoice a finding is about.
 *
 *  **The same invoice is three subjects.** `pr-INV-1` in the register,
 *  `g1-INV-1` in GSTR-1, `2b-INV-1` in GSTR-2B — joining the statement on the
 *  raw subject would file one invoice's findings under three different rows
 *  and none of them would total correctly. */
export function invoiceKey(finding: PackFinding): string {
  const projected = first(evidenceOf(finding), "invoiceNumber");
  if (projected !== "") return projected;
  return localName(finding.subject).replace(/^(pr|g1|2b|receipt|purchase|payment)-/, "");
}

function total(rows: readonly SourceInvoice[], field: "taxableValue" | "taxAmount"): number {
  return rows.reduce((sum, row) => sum + Number(row[field] || 0), 0);
}

export function sourceSummary(rows: readonly SourceInvoice[]): SourceSummary {
  return {
    count: rows.length,
    taxableValue: total(rows, "taxableValue"),
    taxAmount: total(rows, "taxAmount"),
    periods: [...new Set(rows.map((row) => row.period).filter((p) => p !== ""))].sort(),
  };
}

/** The statement's reconciling lines, one per kind of finding.
 *
 *  **Every amount comes from the source rows, never from the finding's own
 *  evidence.** Rules project whichever evidence makes them reviewable —
 *  `SupplierNotFiled` a tax amount, `MissingInBooks` a taxable value,
 *  `Reversed` a flag — so totalling evidence would add different things under
 *  one heading. The invoice number joins back to the source row, which carries
 *  both figures for every invoice in it.
 *
 *  **A dismissed finding is left out.** A human has considered and rejected
 *  it; leaving it in means the statement never converges however much work
 *  gets done. An *accepted* one stays: accepting confirms the item is real,
 *  which is the opposite of closing it. */
export function reconcilingItems(
  findings: readonly PackFinding[],
  books: readonly SourceInvoice[],
  authority: readonly SourceInvoice[],
): ReconcilingItem[] {
  const byNumber = new Map<string, SourceInvoice>();
  for (const row of [...authority, ...books]) byNumber.set(row.invoiceNumber, row);

  const grouped = new Map<string, Set<string>>();
  for (const finding of findings) {
    if (finding.status === "rejected") continue;
    const key = invoiceKey(finding);
    if (key === "") continue;
    const bucket = grouped.get(finding.label);
    if (bucket) bucket.add(key);
    else grouped.set(finding.label, new Set([key]));
  }

  const items: ReconcilingItem[] = [];
  for (const [label, invoices] of grouped) {
    const scenario = scenarioFor(label);
    const numbers = [...invoices].sort();
    const rows = numbers.map(
      (number) =>
        byNumber.get(number) ?? {
          invoiceNumber: number,
          gstin: "",
          supplierName: "",
          invoiceDate: "",
          taxableValue: "0.00",
          taxAmount: "0.00",
          period: "",
        },
    );
    items.push({
      label,
      title: scenario.title,
      sign: scenario.sign,
      count: numbers.length,
      taxableValue: total(rows, "taxableValue"),
      taxAmount: total(rows, "taxAmount"),
      invoices: numbers,
      rows,
    });
  }
  // Signed items first — they are what the difference is made of — then the
  // ones that explain none of it but still stop a claim.
  return items.sort((a, b) => Math.abs(b.sign) - Math.abs(a.sign) || a.title.localeCompare(b.title));
}

/** Below one paisa is zero.
 *
 *  Floating-point addition of rupee figures does not land on exactly zero, and
 *  a statement reading "unexplained ₹0.00" while reporting itself unreconciled
 *  is a bug report from a CA within the hour. */
const PAISA = 0.005;

export function buildStatement(input: {
  books: readonly SourceInvoice[];
  authority: readonly SourceInvoice[];
  findings: readonly PackFinding[];
}): Statement {
  const books = sourceSummary(input.books);
  const authority = sourceSummary(input.authority);
  const items = reconcilingItems(input.findings, input.books, input.authority);

  const difference = {
    taxableValue: books.taxableValue - authority.taxableValue,
    taxAmount: books.taxAmount - authority.taxAmount,
  };
  const explained = {
    taxableValue: items.reduce((sum, item) => sum + item.sign * item.taxableValue, 0),
    taxAmount: items.reduce((sum, item) => sum + item.sign * item.taxAmount, 0),
  };
  const unexplained = {
    taxableValue: difference.taxableValue - explained.taxableValue,
    taxAmount: difference.taxAmount - explained.taxAmount,
  };

  return {
    books,
    authority,
    difference,
    explained,
    unexplained,
    items,
    reconciled: Math.abs(unexplained.taxAmount) < PAISA,
  };
}

/** The same quoting `books.ts` applies on the way in, applied on the way out —
 *  a supplier called `Kumar, Sons & Co` must not shift every column of the
 *  file a CA opens in Excel. */
function csvField(value: string): string {
  return /[",\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

const CSV_HEADER = "Finding,Invoice,GSTIN,Supplier,Invoice date,Period,Taxable value,Tax";

/** The working paper a CA takes away — one row per invoice, with the finding
 *  against it, in the column order the practitioner formats already use. */
export function statementCsv(items: readonly ReconcilingItem[]): string {
  const lines = [CSV_HEADER];
  for (const item of items) {
    for (const row of item.rows) {
      lines.push(
        [
          item.title,
          row.invoiceNumber,
          row.gstin,
          row.supplierName,
          row.invoiceDate,
          row.period,
          row.taxableValue,
          row.taxAmount,
        ]
          .map(csvField)
          .join(","),
      );
    }
  }
  return lines.join("\n");
}
