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
  /** **What two records being the same thing means here**, read from the
   *  predicate the pack names in `[console.reconciliation].match_key`.
   *
   *  The rules join on this; the statement must too. It did not, and the page
   *  showed it: the register writes `INV/1014` and the authority reports
   *  `INV-1014`, the rules matched them and fired nothing, and the statement
   *  listed both as unexplained because it keyed on the printed number. The
   *  residual total stayed *correct* — the two contributions cancel — which is
   *  exactly what let it survive review, with a right number over a list naming
   *  two invoices that reconcile perfectly.
   *
   *  Empty for a pack that declares no key, which falls back to the printed
   *  identity rather than collapsing every record onto one empty string. */
  readonly matchKey: string;
  /** **The four heads, because ITC is claimed head-wise.** Every practitioner
   *  reconciliation format checked carries BASIC | CGST | SGST | IGST | CESS as
   *  separate columns, and GSTR-3B is *filed* that way — so an intra-state
   *  supply booked as inter-state is a real reversal exposure with interest.
   *  A total-only statement cannot show it: moving ₹18,000 from CGST+SGST to
   *  IGST nets to exactly zero on the total, and the invoice reads as a perfect
   *  match. Empty where a source carried no head columns — absent is not zero,
   *  and a register exported without them must still reconcile on totals. */
  readonly igst: string;
  readonly cgst: string;
  readonly sgst: string;
  readonly cess: string;
  readonly period: string;
}

/** The five figures a GST reconciliation is actually done in. */
export interface Heads {
  readonly taxableValue: number;
  readonly taxAmount: number;
  readonly igst: number;
  readonly cgst: number;
  readonly sgst: number;
  readonly cess: number;
}

const HEAD_FIELDS = ["taxableValue", "taxAmount", "igst", "cgst", "sgst", "cess"] as const;

export const ZERO_HEADS: Heads = { taxableValue: 0, taxAmount: 0, igst: 0, cgst: 0, sgst: 0, cess: 0 };

function headsOf(rows: readonly SourceInvoice[]): Heads {
  const out = { ...ZERO_HEADS } as Record<(typeof HEAD_FIELDS)[number], number>;
  for (const row of rows) {
    for (const field of HEAD_FIELDS) out[field] += Number(row[field] || 0);
  }
  return out;
}

/** `a − b`, head by head. */
function minusHeads(a: Heads, b: Heads): Heads {
  return {
    taxableValue: a.taxableValue - b.taxableValue,
    taxAmount: a.taxAmount - b.taxAmount,
    igst: a.igst - b.igst,
    cgst: a.cgst - b.cgst,
    sgst: a.sgst - b.sgst,
    cess: a.cess - b.cess,
  };
}

/** A source row as SPARQL returned it, still carrying the invoice's own
 *  subject — which is what makes duplicates identifiable. */
export interface BoundInvoice extends SourceInvoice {
  readonly subject: string;
}

/** One row per invoice, however many times the query bound it.
 *
 *  **The bug that inflated every total on the page by about 44%.** The source
 *  query carries `OPTIONAL { ?supplier gst:supplierName ?supplierName }`, and
 *  a supplier is named in every document that mentions them — the bundled
 *  fixture, an uploaded GSTR-2A, an uploaded register. Each of those is a
 *  separate named graph, so the OPTIONAL matched several times and the same
 *  invoice came back several times over. Nothing about that looks wrong in a
 *  result table, which is why it reached a screenshot: 26 invoices where the
 *  graph held 18, every figure plausible and every figure false.
 *
 *  **Collapsed on the subject, not the invoice number.** Two suppliers can
 *  legitimately issue the same invoice number, and collapsing on the number
 *  would silently drop one of them — trading an overstatement for an
 *  understatement, which in a tax working paper is the worse direction.
 *
 *  The richest binding wins field by field, because the fan-out is precisely
 *  what carries the supplier name: keeping whichever row arrived first would
 *  fix the totals by throwing away the data the OPTIONAL existed for. */
export function distinctInvoices(rows: readonly BoundInvoice[]): SourceInvoice[] {
  const bySubject = new Map<string, SourceInvoice>();
  for (const row of rows) {
    const seen = bySubject.get(row.subject);
    if (!seen) {
      bySubject.set(row.subject, row);
      continue;
    }
    bySubject.set(row.subject, {
      ...seen,
      supplierName: seen.supplierName || row.supplierName,
      gstin: seen.gstin || row.gstin,
      invoiceDate: seen.invoiceDate || row.invoiceDate,
      period: seen.period || row.period,
    });
  }
  return [...bySubject.values()];
}

export interface SourceSummary extends Heads {
  readonly count: number;
  readonly periods: readonly string[];
}

/** One line of the statement: every finding of one kind, valued by what its
 *  invoices contribute to `books − GSTR-2B`.
 *
 *  **Positive raises the books above the authority, negative lowers it, and
 *  neither is a property of the rule** — it falls out of one subtraction that
 *  covers every case. See {@link contribution}. */
export interface ReconcilingItem {
  readonly label: string;
  readonly title: string;
  readonly count: number;
  readonly taxableValue: number;
  readonly taxAmount: number;
  /** **The credit these invoices carry, which is not the same number as
   *  {@link taxAmount} and must not be shown in its place.** The statement
   *  needs the *contribution* to `books − GSTR-2B`; a reviewer looking at
   *  "unpaid past 180 days" needs the credit at risk. On an invoice both
   *  sides agree on, the contribution is zero and the credit at stake is the
   *  whole of it — reporting ₹0 beside a rule that means "reverse this" would
   *  read as nothing to do. */
  readonly atStake: number;
  readonly invoices: readonly string[];
  readonly rows: readonly SourceInvoice[];
}

export interface Statement {
  readonly books: SourceSummary;
  readonly authority: SourceSummary;
  /** **Head by head, not only as a total.** The difference that matters most
   *  is the one a total cannot show: heads that disagree while summing to the
   *  same figure. */
  readonly difference: Heads;
  readonly explained: { readonly taxableValue: number; readonly taxAmount: number };
  /** The residual, **with the invoices it consists of**. "₹4,500 unaccounted
   *  for" sends a reviewer through the whole period; naming the invoice is a
   *  minute's work. */
  readonly unexplained: {
    readonly taxableValue: number;
    readonly taxAmount: number;
    readonly invoices: readonly SourceInvoice[];
  };
  readonly items: readonly ReconcilingItem[];
  /** Whether the rules account for the whole difference. */
  readonly reconciled: boolean;
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
  readonly tone: "warning" | "danger" | "info";
  /** The rule's own evidence, in words, for the rules whose whole point is a
   *  pair of values.
   *
   *  **Per rule, because a predicate does not identify a fact here.**
   *  `PaymentOverdue` projects `gst:atTime` twice — once for the purchase and
   *  once for the payment — and `GoodsReceiptTiming` projects it once, for the
   *  receipt. Rendering by predicate name alone labelled a purchase date as
   *  "received", which is not a formatting slip: it is the page stating a fact
   *  about somebody's tax position that is false. The rule knows what its own
   *  evidence means, and the order is the order `pack.toml` projected it in. */
  readonly detail?: (evidence: EvidenceIndex) => string;
}

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
export function localName(term: string): string {
  const slash = Math.max(term.lastIndexOf("#"), term.lastIndexOf("/"));
  const cut = slash >= 0 ? slash : term.lastIndexOf(":");
  const tail = cut >= 0 ? term.slice(cut + 1) : term;
  return tail.length > 0 ? tail : term;
}

/** Guidance keyed by rule label, as the pack declared it.
 *
 *  **This was a table of thirteen GST rules compiled into the console.** It is
 *  now `[findings.guidance]` in `pack.toml`, delivered by
 *  `GET /packs/{pack}/finding-rules`, so a healthcare or banking rule carries
 *  its own words rather than rendering under GST's or under none. */
export type GuidanceIndex = Readonly<Record<string, Scenario | undefined>>;

export function scenarioFor(label: string, guidance: GuidanceIndex = {}): Scenario {
  const declared = guidance[label];
  if (declared) return declared;
  // A rule whose pack declared no guidance must still render: the label beats
  // an empty card, and beats a crash by a great deal more.
  const name = localName(label);
  return { title: name, meaning: "", nextAction: `Review ${name}.`, tone: "info" };
}

/** The guidance a pack's registered rules carry, indexed by label. */
export function guidanceFrom(
  rules: readonly { readonly label: string; readonly guidance?: unknown }[],
): GuidanceIndex {
  const out: Record<string, Scenario> = {};
  for (const rule of rules) {
    const g = rule.guidance as Partial<Scenario> | undefined;
    if (!g?.title) continue;
    out[rule.label] = {
      title: g.title,
      meaning: g.meaning ?? "",
      nextAction: g.nextAction ?? "",
      tone: g.tone ?? "info",
    };
  }
  return out;
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

export function sourceSummary(rows: readonly SourceInvoice[]): SourceSummary {
  return {
    count: rows.length,
    ...headsOf(rows),
    periods: [...new Set(rows.map((row) => row.period).filter((p) => p !== ""))].sort(),
  };
}

/** The one value meaning "every period" in the period filter. A shared
 *  constant, so the Select's option and the filter function cannot disagree
 *  about the word that means "no filter". */
export const ALL_PERIODS = "all";

/** The distinct filing periods across the given sources, sorted — the union
 *  the period filter offers. Empty when nothing is loaded. */
export function periodsOf(...sources: readonly (readonly SourceInvoice[])[]): readonly string[] {
  const seen = new Set<string>();
  for (const rows of sources) {
    for (const row of rows) {
      if (row.period !== "") seen.add(row.period);
    }
  }
  return [...seen].sort();
}

/** Rows narrowed to one filing period.
 *
 *  **`"all"` returns the array itself, unchanged** — no filter must behave
 *  exactly like the workspace did before the filter existed, including a
 *  record that shipped without a period. A source exported without a period
 *  column still reconciles on totals, and its rupees must never vanish from
 *  "all" because an earlier filter dropped them. */
export function forPeriod(rows: readonly SourceInvoice[], period: string): readonly SourceInvoice[] {
  if (period === ALL_PERIODS || period === "") return rows;
  return rows.filter((row) => row.period === period);
}

/** Findings narrowed to a period, by joining on the invoices the narrowed rows
 *  still hold.
 *
 *  A finding is about an invoice, so the period of a finding is the period of
 *  that invoice — and the join uses both the declared key and the printed
 *  identity, exactly as the statement's own join does, so a finding cannot
 *  survive into a period that no longer holds the row it explains. */
export function findingsForPeriod(
  findings: readonly PackFinding[],
  books: readonly SourceInvoice[],
  authority: readonly SourceInvoice[],
): readonly PackFinding[] {
  const held = new Set<string>();
  for (const row of [...books, ...authority]) {
    held.add(keyOf(row));
    if (row.invoiceNumber !== "") held.add(row.invoiceNumber);
  }
  return findings.filter((finding) => held.has(invoiceKey(finding)));
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
/** An invoice indexed by number, per side, so absence is answerable. */
function index(rows: readonly SourceInvoice[]): Map<string, SourceInvoice> {
  const found = new Map<string, SourceInvoice>();
  for (const row of rows) {
    found.set(keyOf(row), row);
    // Also reachable by its printed identity, because a *finding* projects the
    // number a human reads, not the key the rules joined on.
    if (row.invoiceNumber !== "") found.set(row.invoiceNumber, row);
  }
  return found;
}

/** What a row is joined by: the pack's declared key, or its printed identity
 *  when the pack declares none. */
function keyOf(row: SourceInvoice): string {
  return row.matchKey !== "" ? row.matchKey : row.invoiceNumber;
}

const ZERO: SourceInvoice = {
  invoiceNumber: "",
  matchKey: "",
  gstin: "",
  supplierName: "",
  invoiceDate: "",
  taxableValue: "0.00",
  taxAmount: "0.00",
  igst: "0.00",
  cgst: "0.00",
  sgst: "0.00",
  cess: "0.00",
  period: "",
};

/** What one invoice contributes to `books − GSTR-2B`.
 *
 *  **One subtraction covers every case, which is why there is no sign per
 *  rule.** An invoice only in the register contributes its whole value; one
 *  only in GSTR-2B contributes its negative; one present in both contributes
 *  the difference between them — which is exactly what a mismatch rule has
 *  found, and which a fixed sign of 0 threw away. A live run made that
 *  concrete: real value differences the rules *had* found were reported as
 *  unaccounted for on the same screen, immediately below the findings that
 *  accounted for them. */
function contribution(
  number: string,
  booksBy: Map<string, SourceInvoice>,
  authorityBy: Map<string, SourceInvoice>,
): { taxableValue: number; taxAmount: number } {
  const mine = booksBy.get(number) ?? ZERO;
  const theirs = authorityBy.get(number) ?? ZERO;
  return {
    taxableValue: Number(mine.taxableValue || 0) - Number(theirs.taxableValue || 0),
    taxAmount: Number(mine.taxAmount || 0) - Number(theirs.taxAmount || 0),
  };
}

/** Which invoices each kind of finding is about, **as join keys**.
 *
 *  A rule projects the identity a human reads (`INV/1014`); the statement joins
 *  on the key the pack declares (`INV1014`). Resolving here rather than at each
 *  call site is what stops the two drifting — they already did once, in both
 *  directions: first the statement keyed on the printed number and listed two
 *  spellings of one invoice as unexplained, then it keyed on the declared key
 *  and listed every invoice that *had* a finding as unexplained instead. */
function invoicesByLabel(
  findings: readonly PackFinding[],
  resolve: (identity: string) => string,
): Map<string, Set<string>> {
  const grouped = new Map<string, Set<string>>();
  for (const finding of findings) {
    // A dismissed finding has been considered and rejected by a human; leaving
    // it in means the statement never converges however much work gets done.
    // An *accepted* one stays — accepting confirms the item is real, which is
    // the opposite of closing it.
    if (finding.status === "rejected") continue;
    const identity = invoiceKey(finding);
    if (identity === "") continue;
    const key = resolve(identity);
    const bucket = grouped.get(finding.label);
    if (bucket) bucket.add(key);
    else grouped.set(finding.label, new Set([key]));
  }
  return grouped;
}

export function reconcilingItems(
  findings: readonly PackFinding[],
  books: readonly SourceInvoice[],
  authority: readonly SourceInvoice[],
  guidance: GuidanceIndex = {},
): ReconcilingItem[] {
  const booksBy = index(books);
  const authorityBy = index(authority);

  // A rule's projected identity resolved to the row it belongs to, and thence
  // to that row's join key. An identity matching no row keeps its own value —
  // a finding about a record neither source holds is still a finding.
  const resolve = (identity: string) => {
    const row = booksBy.get(identity) ?? authorityBy.get(identity);
    return row ? keyOf(row) : identity;
  };

  const items: ReconcilingItem[] = [];
  for (const [label, invoices] of invoicesByLabel(findings, resolve)) {
    const numbers = [...invoices].sort();
    const totals = numbers.reduce(
      (sum, number) => {
        const one = contribution(number, booksBy, authorityBy);
        return { taxableValue: sum.taxableValue + one.taxableValue, taxAmount: sum.taxAmount + one.taxAmount };
      },
      { taxableValue: 0, taxAmount: 0 },
    );
    items.push({
      label,
      title: scenarioFor(label, guidance).title,
      count: numbers.length,
      taxableValue: totals.taxableValue,
      taxAmount: totals.taxAmount,
      atStake: numbers.reduce(
        (sum, number) => sum + Number((booksBy.get(number) ?? authorityBy.get(number))?.taxAmount || 0),
        0,
      ),
      invoices: numbers,
      // The register's own row is shown when there is one — it is the
      // taxpayer's record of the invoice, and the one they will go and check.
      // Only an invoice they never booked falls back to the authority's.
      rows: numbers.map((number) => booksBy.get(number) ?? authorityBy.get(number) ?? { ...ZERO, invoiceNumber: number }),
    });
  }
  // Largest movement first: the line worth reading is the one carrying most of
  // the difference, not the one whose rule happens to sort first.
  return items.sort((a, b) => Math.abs(b.taxAmount) - Math.abs(a.taxAmount) || a.title.localeCompare(b.title));
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
  guidance?: GuidanceIndex;
}): Statement {
  const books = sourceSummary(input.books);
  const authority = sourceSummary(input.authority);
  const items = reconcilingItems(input.findings, input.books, input.authority, input.guidance);

  const booksBy = index(input.books);
  const authorityBy = index(input.authority);

  const difference = minusHeads(books, authority);

  // **Over the union of invoices, not the sum of the lines.** One invoice
  // routinely fires two rules — `AmountMismatch` compares the register against
  // GSTR-2B and `BooksGstr1Mismatch` against GSTR-1, and a single wrong figure
  // trips both. Adding the lines would explain the same rupees twice and drive
  // the residual negative, which reads as the reconciliation having found more
  // than exists.
  const named = new Set(items.flatMap((item) => item.invoices));
  const explained = [...named].reduce(
    (sum, number) => {
      const one = contribution(number, booksBy, authorityBy);
      return { taxableValue: sum.taxableValue + one.taxableValue, taxAmount: sum.taxAmount + one.taxAmount };
    },
    { taxableValue: 0, taxAmount: 0 },
  );

  // Every invoice that moves the difference and that no rule named. An invoice
  // both sides agree on contributes nothing and is not listed — in a healthy
  // period that is almost all of them, and listing them would bury the few
  // that matter.
  const unexplainedInvoices: SourceInvoice[] = [];
  // **Over the join keys, not every alias.** `index` deliberately makes a row
  // reachable by both its key and its printed number so a finding can find it;
  // walking those keys here would visit the same invoice twice and report it
  // as two unexplained records — which is the bug this whole key exists to fix.
  const joinKeys = new Set([...input.books, ...input.authority].map(keyOf));
  for (const number of joinKeys) {
    if (named.has(number)) continue;
    const one = contribution(number, booksBy, authorityBy);
    if (Math.abs(one.taxAmount) < PAISA && Math.abs(one.taxableValue) < PAISA) continue;
    const row = booksBy.get(number) ?? authorityBy.get(number);
    if (row) unexplainedInvoices.push(row);
  }
  unexplainedInvoices.sort((a, b) => a.invoiceNumber.localeCompare(b.invoiceNumber));

  const unexplained = {
    taxableValue: difference.taxableValue - explained.taxableValue,
    taxAmount: difference.taxAmount - explained.taxAmount,
    invoices: unexplainedInvoices,
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

/** One supplier, and which of the three documents describe them.
 *
 *  **This is the graph's own model made visible, not a report over three
 *  spreadsheets.** A supplier is one `gst:Supplier` subject that the register,
 *  the GSTR-2A and the GSTR-2B all point at with `gst:issuedBy` — the shared
 *  subject Epic 105c introduced precisely so a third source *enriches* a node
 *  rather than creating a fourth copy of a GSTIN string. Because it is one
 *  subject, "which of my suppliers has filed nothing at all" is answerable by
 *  asking which documents describe it. */
export interface SupplierRow {
  readonly gstin: string;
  readonly supplierName: string;
  readonly inBooks: boolean;
  readonly inGstr1: boolean;
  readonly inAuthority: boolean;
  readonly invoices: number;
  readonly booksTax: number;
  readonly authorityTax: number;
}

/** The supplier-level view of the same reconciliation.
 *
 *  **Chasing is done supplier by supplier, not invoice by invoice**, which is
 *  why every practitioner tool offers this view: one phone call covers every
 *  invoice a supplier has failed to file, and the order to make those calls in
 *  is by credit at stake.
 *
 *  Computed from the three source lists already fetched rather than a fourth
 *  query — the answer is identical and it cannot disagree with the totals
 *  rendered beside it, which a separately-fetched figure eventually would. */
export function supplierView(sources: {
  books: readonly SourceInvoice[];
  gstr1: readonly SourceInvoice[];
  authority: readonly SourceInvoice[];
}): SupplierRow[] {
  const rows = new Map<string, {
    gstin: string;
    supplierName: string;
    inBooks: boolean;
    inGstr1: boolean;
    inAuthority: boolean;
    invoices: number;
    booksTax: number;
    authorityTax: number;
  }>();

  const visit = (invoice: SourceInvoice, where: "books" | "gstr1" | "authority") => {
    if (invoice.gstin === "") return;
    const row =
      rows.get(invoice.gstin) ??
      {
        gstin: invoice.gstin,
        supplierName: "",
        inBooks: false,
        inGstr1: false,
        inAuthority: false,
        invoices: 0,
        booksTax: 0,
        authorityTax: 0,
      };
    // The name lives on the shared subject and only GSTR-2A carries it in this
    // pack, so it has to survive being absent from the other two sources.
    if (row.supplierName === "") row.supplierName = invoice.supplierName;
    if (where === "books") {
      row.inBooks = true;
      row.invoices += 1;
      row.booksTax += Number(invoice.taxAmount || 0);
    } else if (where === "gstr1") {
      row.inGstr1 = true;
    } else {
      row.inAuthority = true;
      row.authorityTax += Number(invoice.taxAmount || 0);
    }
    rows.set(invoice.gstin, row);
  };

  for (const invoice of sources.books) visit(invoice, "books");
  for (const invoice of sources.gstr1) visit(invoice, "gstr1");
  for (const invoice of sources.authority) visit(invoice, "authority");

  // Largest credit first: that is the order to make the calls in.
  return [...rows.values()].sort(
    (a, b) => Math.max(b.booksTax, b.authorityTax) - Math.max(a.booksTax, a.authorityTax),
  );
}
