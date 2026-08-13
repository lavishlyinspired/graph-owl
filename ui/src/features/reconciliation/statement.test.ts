/** The reconciliation statement, as a CA reads one — Plan 108 Slice 7.
 *
 *  **The arithmetic is the risk here, not the rendering.** A wrong figure in a
 *  tax working paper is worse than no figure, because it will be signed off.
 *  So every number this module produces is either summed from the graph's own
 *  values or explicitly reported as unexplained — there is no bucket that
 *  quietly absorbs a residual.
 *
 *  The expected values below are **hand-derived from the fixture rows in the
 *  test itself**, not copied from a run. A test whose expectations came from
 *  the implementation asserts only that the implementation has not changed. */

import { describe, expect, it } from "vitest";
import type { PackFinding } from "../../api";
import {
  buildStatement,
  distinctInvoices,
  evidenceOf,
  invoiceKey,
  values,
  reconcilingItems,
  scenarioFor,
  sourceSummary,
  statementCsv,
  type SourceInvoice,
} from "./statement";

function invoice(overrides: Partial<SourceInvoice> & { invoiceNumber: string }): SourceInvoice {
  return {
    gstin: "27AABCU9603R1ZM",
    supplierName: "Umbrella",
    invoiceDate: "2026-07-04",
    taxableValue: "100000.00",
    taxAmount: "18000.00",
    period: "2026-07",
    ...overrides,
  };
}

function finding(label: string, subject: string, evidence: Record<string, string>): PackFinding {
  return {
    id: `${label}-${subject}`,
    pack: "gst",
    label,
    subject: `https://graph-owl.dev/packs/gst#${subject}`,
    summary: "",
    governedBy: "gst:Section16",
    evidence: Object.entries(evidence).map(([predicate, value]) => ({
      subject,
      predicate: `https://graph-owl.dev/packs/gst#${predicate}`,
      value,
    })),
    status: "pending",
    detectedAt: "2026-08-13T00:00:00Z",
  };
}

describe("reading a finding's own evidence", () => {
  it("indexes evidence by the predicate it came from", () => {
    const found = evidenceOf(finding("gst:SupplierNotFiled", "pr-INV-1", { invoiceNumber: "INV-1", taxAmount: "900" }));

    expect(values(found, "invoiceNumber")).toEqual(["INV-1"]);
    expect(values(found, "taxAmount")).toEqual(["900"]);
  });

  /** **Two facts under one predicate is the normal case, not an edge one.**
   *  `AmountMismatch` projects the register's `taxableValue` *and* the
   *  authority's, both as `gst:taxableValue` — that is what the finding is
   *  about. Keeping only one would silently report a mismatch with a single
   *  number, which is unreadable and unreviewable. */
  it("keeps both values when one predicate carries two", () => {
    const found = evidenceOf(
      finding("gst:AmountMismatch", "pr-INV-2", { invoiceNumber: "INV-2" }),
    );
    const both = evidenceOf({
      ...finding("gst:AmountMismatch", "pr-INV-2", {}),
      evidence: [
        { subject: "x", predicate: "gst:taxableValue", value: "100000.00" },
        { subject: "x", predicate: "gst:taxableValue", value: "95000.00" },
      ],
    });

    expect(values(found, "invoiceNumber")).toEqual(["INV-2"]);
    expect(values(both, "taxableValue")).toEqual(["100000.00", "95000.00"]);
  });

  it("reports nothing rather than throwing for a predicate the rule never projected", () => {
    expect(values(evidenceOf(finding("gst:Reversed", "pr-INV-3", {})), "taxAmount")).toEqual([]);
  });
});

describe("which invoice a finding is about", () => {
  it("takes the invoice number the rule projected", () => {
    expect(invoiceKey(finding("gst:SupplierNotFiled", "pr-INV-1", { invoiceNumber: "INV-1" }))).toBe("INV-1");
  });

  /** **The subject's local name is the fallback, with its source prefix
   *  stripped.** The same invoice is `pr-INV-1` in the register, `g1-INV-1` in
   *  GSTR-1 and `2b-INV-1` in GSTR-2B — three subjects, one invoice. Joining
   *  the statement on the raw subject would file one invoice's findings under
   *  three different rows. */
  it("falls back to the subject with its source prefix stripped", () => {
    expect(invoiceKey(finding("gst:MissingInBooks", "g1-INV-8", {}))).toBe("INV-8");
    expect(invoiceKey(finding("gst:PotentialMismatch", "2b-INV-9", {}))).toBe("INV-9");
  });
});

describe("what each source holds", () => {
  it("counts and totals the rows", () => {
    const summary = sourceSummary([
      invoice({ invoiceNumber: "INV-1", taxableValue: "100000.00", taxAmount: "18000.00" }),
      invoice({ invoiceNumber: "INV-2", taxableValue: "50000.00", taxAmount: "9000.00" }),
    ]);

    expect(summary.count).toBe(2);
    expect(summary.taxableValue).toBe(150000);
    expect(summary.taxAmount).toBe(27000);
  });

  it("reports an empty source as empty rather than as zero rupees of something", () => {
    expect(sourceSummary([])).toEqual({ count: 0, taxableValue: 0, taxAmount: 0, periods: [] });
  });

  /** A CA reconciles one period at a time, and uploading August's 2B against
   *  July's books is a mistake worth seeing before running anything. */
  it("names the distinct periods a source covers, in order", () => {
    const summary = sourceSummary([
      invoice({ invoiceNumber: "INV-2", period: "2026-08" }),
      invoice({ invoiceNumber: "INV-1", period: "2026-07" }),
      invoice({ invoiceNumber: "INV-3", period: "2026-08" }),
    ]);

    expect(summary.periods).toEqual(["2026-07", "2026-08"]);
  });
});

describe("the reconciling items between books and GSTR-2B", () => {
  const books = [
    invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00", taxableValue: "100000.00" }),
    invoice({ invoiceNumber: "INV-3", taxAmount: "4500.00", taxableValue: "25000.00" }),
    invoice({ invoiceNumber: "INV-7", taxAmount: "5400.00", taxableValue: "30000.00" }),
  ];
  const authority = [
    invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00", taxableValue: "100000.00" }),
    invoice({ invoiceNumber: "INV-8", taxAmount: "8100.00", taxableValue: "45000.00" }),
  ];
  const findings = [
    finding("gst:SupplierNotFiled", "pr-INV-3", { invoiceNumber: "INV-3" }),
    finding("gst:Gstr1NotIn2b", "pr-INV-7", { invoiceNumber: "INV-7" }),
    finding("gst:MissingInBooks", "g1-INV-8", { invoiceNumber: "INV-8" }),
  ];

  /** **The amount comes from the graph, never from the finding.** Rules
   *  project whichever evidence makes them reviewable — `SupplierNotFiled`
   *  carries a tax amount, `MissingInBooks` a taxable value, `Reversed`
   *  neither in a form that sums — so totalling the evidence would total
   *  different things under one heading. The invoice number joins back to the
   *  source row, which carries both figures for every invoice. */
  it("values each item from the source rows, not from the finding's evidence", () => {
    const items = reconcilingItems(findings, books, authority);
    const notFiled = items.find((i) => i.label === "gst:SupplierNotFiled")!;

    expect(notFiled.count).toBe(1);
    expect(notFiled.taxAmount).toBe(4500);
    expect(notFiled.taxableValue).toBe(25000);
  });

  /** **An item's amount is what it contributes to `books \u2212 GSTR-2B`, and
   *  that is one subtraction covering every case rather than a sign per rule.**
   *  An invoice only in the books contributes its whole value, one only in 2B
   *  contributes its negative, and one present in both contributes the
   *  difference — so a rule about invoices that *match* and still disagree
   *  carries its delta automatically instead of being classed as explaining
   *  nothing.
   *
   *  A sign per rule was the first attempt, and a live run showed why it was
   *  wrong: the mismatch rules were hard-coded to 0, so real value differences
   *  the rules had found were reported as unaccounted for, on the same screen,
   *  directly below the findings that accounted for them. */
  it("values an item by what it contributes to books minus GSTR-2B", () => {
    const items = reconcilingItems(findings, books, authority);

    expect(items.find((i) => i.label === "gst:SupplierNotFiled")!.taxAmount).toBe(4500);
    expect(items.find((i) => i.label === "gst:MissingInBooks")!.taxAmount).toBe(-8100);
  });

  it("gives a matched-but-disagreeing invoice its delta, not zero", () => {
    const items = reconcilingItems(
      [finding("gst:AmountMismatch", "pr-INV-1", { invoiceNumber: "INV-1" })],
      [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })],
      [invoice({ invoiceNumber: "INV-1", taxAmount: "17100.00" })],
    );

    expect(items[0]!.taxAmount).toBe(900);
  });

  it("groups every finding of one kind into one line", () => {
    const items = reconcilingItems(
      [...findings, finding("gst:SupplierNotFiled", "pr-INV-1", { invoiceNumber: "INV-1" })],
      books,
      authority,
    );

    expect(items.filter((i) => i.label === "gst:SupplierNotFiled")).toHaveLength(1);
    expect(items.find((i) => i.label === "gst:SupplierNotFiled")!.count).toBe(2);
  });

  it("lists the invoices behind a line so a reviewer can open them", () => {
    const items = reconcilingItems(findings, books, authority);

    expect(items.find((i) => i.label === "gst:Gstr1NotIn2b")!.invoices).toEqual(["INV-7"]);
  });

  /** A dismissed finding has been considered and rejected by a human; leaving
   *  it in the statement means the total never converges however much work
   *  gets done. */
  it("leaves a dismissed finding out of the statement", () => {
    const dismissed = findings.map((f) =>
      f.label === "gst:SupplierNotFiled" ? { ...f, status: "rejected" as const } : f,
    );

    expect(reconcilingItems(dismissed, books, authority).some((i) => i.label === "gst:SupplierNotFiled")).toBe(false);
  });

  it("keeps an accepted finding, which is a confirmed reconciling item, not a closed one", () => {
    const accepted = findings.map((f) =>
      f.label === "gst:SupplierNotFiled" ? { ...f, status: "accepted" as const } : f,
    );

    expect(reconcilingItems(accepted, books, authority).some((i) => i.label === "gst:SupplierNotFiled")).toBe(true);
  });
});

describe("the statement as a whole", () => {
  /** Hand-derived, so the test knows the answer before the code runs:
   *
   *    books    = 18000 + 4500 + 5400 = 27,900   over 3 invoices
   *    2B       = 18000 + 8100        = 26,100   over 2 invoices
   *    difference                     =  1,800
   *    explained: +4500 (INV-3 not filed) +5400 (INV-7 not reached) −8100 (INV-8 not booked)
   *             =  1,800
   *    unexplained                    =      0
   */
  const books = [
    invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00", taxableValue: "100000.00" }),
    invoice({ invoiceNumber: "INV-3", taxAmount: "4500.00", taxableValue: "25000.00" }),
    invoice({ invoiceNumber: "INV-7", taxAmount: "5400.00", taxableValue: "30000.00" }),
  ];
  const authority = [
    invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00", taxableValue: "100000.00" }),
    invoice({ invoiceNumber: "INV-8", taxAmount: "8100.00", taxableValue: "45000.00" }),
  ];
  const findings = [
    finding("gst:SupplierNotFiled", "pr-INV-3", { invoiceNumber: "INV-3" }),
    finding("gst:Gstr1NotIn2b", "pr-INV-7", { invoiceNumber: "INV-7" }),
    finding("gst:MissingInBooks", "g1-INV-8", { invoiceNumber: "INV-8" }),
  ];

  it("opens at the books and closes at GSTR-2B", () => {
    const statement = buildStatement({ books, authority, findings });

    expect(statement.books.taxAmount).toBe(27900);
    expect(statement.authority.taxAmount).toBe(26100);
    expect(statement.difference.taxAmount).toBe(1800);
  });

  it("accounts for the whole difference when every item is explained", () => {
    const statement = buildStatement({ books, authority, findings });

    expect(statement.explained.taxAmount).toBe(1800);
    expect(statement.unexplained.taxAmount).toBe(0);
    expect(statement.reconciled).toBe(true);
  });

  /** **The line that makes this worth more than a spreadsheet.** A residual
   *  the rules cannot explain is the honest answer, and it is what tells a CA
   *  there is something the reconciliation has not found — silently folding it
   *  into a bucket would produce a statement that always balances and never
   *  means anything. */
  it("reports a residual no rule accounts for rather than absorbing it", () => {
    const statement = buildStatement({
      books,
      authority,
      findings: findings.filter((f) => f.label !== "gst:Gstr1NotIn2b"),
    });

    expect(statement.explained.taxAmount).toBe(-3600);
    expect(statement.unexplained.taxAmount).toBe(5400);
    expect(statement.reconciled).toBe(false);
  });

  /** Floating-point addition of rupee figures does not land on zero — `0.10 +
   *  0.20` is `0.30000000000000004` — and a statement reading "unexplained
   *  ₹0.00" while reporting itself unreconciled is a bug report from a CA
   *  within the hour.
   *
   *  INV-2 is in the books and in no GSTR-2B, which is what `SupplierNotFiled`
   *  actually means; the residual is then pure floating-point noise. */
  it("treats a sub-paisa residual as reconciled", () => {
    const statement = buildStatement({
      books: [invoice({ invoiceNumber: "INV-1", taxAmount: "0.10" }), invoice({ invoiceNumber: "INV-2", taxAmount: "0.20" })],
      authority: [invoice({ invoiceNumber: "INV-1", taxAmount: "0.10" })],
      findings: [finding("gst:SupplierNotFiled", "pr-INV-2", { invoiceNumber: "INV-2" })],
    });

    expect(statement.unexplained.taxAmount).not.toBe(0);
    expect(statement.reconciled).toBe(true);
  });

  it("reconciles trivially when both sides are empty", () => {
    const statement = buildStatement({ books: [], authority: [], findings: [] });

    expect(statement.reconciled).toBe(true);
    expect(statement.difference.taxAmount).toBe(0);
  });

  /** **One invoice, two findings, counted once.** INV-1002 in the real pack
   *  fires both `AmountMismatch` (books vs GSTR-2B) and `BooksGstr1Mismatch`
   *  (books vs GSTR-1) — the same ₹900 of difference, found twice. Summing the
   *  lines would explain ₹1,800 of an ₹900 gap and drive the residual
   *  negative, which reads as the reconciliation having found *more* than
   *  exists. The explained total is over the union of invoices, not the sum of
   *  the lines. */
  it("counts an invoice once however many rules named it", () => {
    const statement = buildStatement({
      books: [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })],
      authority: [invoice({ invoiceNumber: "INV-1", taxAmount: "17100.00" })],
      findings: [
        finding("gst:AmountMismatch", "pr-INV-1", { invoiceNumber: "INV-1" }),
        finding("gst:BooksGstr1Mismatch", "pr-INV-1", { invoiceNumber: "INV-1" }),
      ],
    });

    expect(statement.difference.taxAmount).toBe(900);
    expect(statement.explained.taxAmount).toBe(900);
    expect(statement.reconciled).toBe(true);
  });

  /** **The residual has to name itself to be worth printing.** "₹4,500
   *  unaccounted for" sends a CA looking through the whole period; "₹4,500,
   *  and it is INV-9" is a minute's work. */
  it("names the invoices the residual consists of", () => {
    const statement = buildStatement({
      books: [
        invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" }),
        invoice({ invoiceNumber: "INV-9", taxAmount: "4500.00" }),
      ],
      authority: [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })],
      findings: [],
    });

    expect(statement.unexplained.taxAmount).toBe(4500);
    expect(statement.unexplained.invoices.map((row) => row.invoiceNumber)).toEqual(["INV-9"]);
  });

  it("names nothing when every contributing invoice has a finding", () => {
    const statement = buildStatement({
      books: [invoice({ invoiceNumber: "INV-9", taxAmount: "4500.00" })],
      authority: [],
      findings: [finding("gst:SupplierNotFiled", "pr-INV-9", { invoiceNumber: "INV-9" })],
    });

    expect(statement.unexplained.invoices).toEqual([]);
    expect(statement.reconciled).toBe(true);
  });

  /** An invoice both sides agree on contributes nothing and must not be listed
   *  as an unexplained difference just because no rule mentioned it — most
   *  invoices in a healthy period are exactly this. */
  it("does not list a perfectly matched invoice as unexplained", () => {
    const statement = buildStatement({
      books: [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })],
      authority: [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })],
      findings: [],
    });

    expect(statement.unexplained.invoices).toEqual([]);
    expect(statement.reconciled).toBe(true);
  });
});

describe("what each finding means to a CA", () => {
  /** The rules' own summaries are written for a reviewer of the *rule*. A CA
   *  looking at a queue needs the next action, and the two are different
   *  sentences. */
  it("gives every shipped rule a next action", () => {
    for (const label of [
      "gst:SupplierNotFiled",
      "gst:Gstr1NotIn2b",
      "gst:MissingInBooks",
      "gst:BooksGstr1Mismatch",
      "gst:GoodsReceiptTiming",
      "gst:PotentialMismatch",
      "gst:AmountMismatch",
      "gst:ITCNotAvailable",
      "gst:Reversed",
      "gst:GstinTransposition",
      "gst:PaymentOverdue",
    ]) {
      expect(scenarioFor(label).nextAction.length).toBeGreaterThan(0);
    }
  });

  /** A pack this console has never seen must still render. Falling back to the
   *  label beats an empty card, and beats a crash by more. */
  it("falls back for a label it has never seen", () => {
    const unknown = scenarioFor("hospitality:DuplicateGuest");

    expect(unknown.title).toBe("DuplicateGuest");
    expect(unknown.nextAction).toContain("DuplicateGuest");
  });
});

describe("the working paper a CA takes away", () => {
  it("writes one row per invoice with the finding against it", () => {
    const csv = statementCsv([
      {
        label: "gst:SupplierNotFiled",
        title: "Supplier has not filed",
        count: 1,
        taxableValue: 25000,
        taxAmount: 4500,
        atStake: 4500,
        invoices: ["INV-3"],
        rows: [invoice({ invoiceNumber: "INV-3", taxableValue: "25000.00", taxAmount: "4500.00" })],
      },
    ]);

    expect(csv.split("\n")[0]).toBe("Finding,Invoice,GSTIN,Supplier,Invoice date,Period,Taxable value,Tax");
    expect(csv).toContain("Supplier has not filed,INV-3,27AABCU9603R1ZM,Umbrella,2026-07-04,2026-07,25000.00,4500.00");
  });

  /** The same failure `books.ts` guards against on the way in, on the way out:
   *  a supplier called `Kumar, Sons & Co` must not shift every column of the
   *  file a CA opens in Excel. */
  it("quotes a field containing the delimiter", () => {
    const csv = statementCsv([
      {
        label: "gst:SupplierNotFiled",
        title: "Supplier has not filed",
        count: 1,
        taxableValue: 100,
        taxAmount: 18,
        atStake: 18,
        invoices: ["INV-3"],
        rows: [invoice({ invoiceNumber: "INV-3", supplierName: "Kumar, Sons & Co" })],
      },
    ]);

    expect(csv).toContain('"Kumar, Sons & Co"');
  });

  it("writes a header and nothing else when there is nothing to report", () => {
    expect(statementCsv([]).split("\n")).toHaveLength(1);
  });
});

describe("the credit at stake, which is not the contribution to the difference", () => {
  /** **These are two different numbers and showing one for the other is a
   *  wrong figure in a tax working paper.** An invoice both sides agree on
   *  contributes nothing to `books − GSTR-2B`, and the credit riding on it is
   *  its whole tax. `ITCNotAvailable` and `PaymentOverdue` fire on exactly
   *  those invoices — a card reading ₹0.00 beside "reverse this credit" says
   *  there is nothing to do. */
  it("reports the whole credit on an invoice both sides agree on", () => {
    const both = [invoice({ invoiceNumber: "INV-1", taxAmount: "18000.00" })];
    const item = reconcilingItems(
      [finding("gst:PaymentOverdue", "purchase-INV-1", { invoiceNumber: "INV-1" })],
      both,
      both,
    )[0]!;

    expect(item.taxAmount).toBe(0);
    expect(item.atStake).toBe(18000);
  });

  it("reports the authority's figure when the invoice was never booked", () => {
    const item = reconcilingItems(
      [finding("gst:MissingInBooks", "g1-INV-8", { invoiceNumber: "INV-8" })],
      [],
      [invoice({ invoiceNumber: "INV-8", taxAmount: "8100.00" })],
    )[0]!;

    expect(item.taxAmount).toBe(-8100);
    expect(item.atStake).toBe(8100);
  });
});

describe("one row per invoice, however many times the graph binds it", () => {
  /** **The bug that inflated every total on the page by about 44%, found by
   *  comparing the rendered counts against the graph's own.** The source query
   *  carries `OPTIONAL { ?supplier gst:supplierName ?supplierName }`, and a
   *  supplier is named in every document that mentions them — the bundled
   *  fixture, the uploaded GSTR-2A, the uploaded register. Each of those is a
   *  separate named graph, so the OPTIONAL matched several times and SPARQL
   *  returned the same invoice several times over.
   *
   *  Nothing about that looks wrong in a result table, which is why it reached
   *  a screenshot: 26 invoices where the graph held 18, ₹4,82,930 where it held
   *  ₹3,06,710 — every figure plausible and every figure false.
   *
   *  **Deduplicated on the invoice's own subject, not its number.** Two
   *  different suppliers can legitimately issue the same invoice number, and
   *  collapsing on the number would silently drop one of them — trading an
   *  overstatement for an understatement, which in a tax working paper is the
   *  worse direction. */
  it("collapses repeated bindings of one invoice", () => {
    const rows = [
      { subject: "gst:pr-INV-1", ...invoice({ invoiceNumber: "INV-1", supplierName: "" }) },
      { subject: "gst:pr-INV-1", ...invoice({ invoiceNumber: "INV-1", supplierName: "Umbrella" }) },
      { subject: "gst:pr-INV-2", ...invoice({ invoiceNumber: "INV-2" }) },
    ];

    expect(distinctInvoices(rows).map((row) => row.invoiceNumber)).toEqual(["INV-1", "INV-2"]);
  });

  /** The fan-out is what carries the supplier name, so collapsing must not
   *  throw it away by keeping whichever row happened to arrive first. */
  it("keeps a supplier name that only one of the repeated bindings carried", () => {
    const rows = [
      { subject: "gst:pr-INV-1", ...invoice({ invoiceNumber: "INV-1", supplierName: "" }) },
      { subject: "gst:pr-INV-1", ...invoice({ invoiceNumber: "INV-1", supplierName: "Umbrella" }) },
    ];

    expect(distinctInvoices(rows)[0]!.supplierName).toBe("Umbrella");
  });

  it("keeps two suppliers' identically numbered invoices apart", () => {
    const rows = [
      { subject: "gst:pr-a", ...invoice({ invoiceNumber: "INV-001", gstin: "27AAAAA0000A1Z5" }) },
      { subject: "gst:pr-b", ...invoice({ invoiceNumber: "INV-001", gstin: "29BBBBB1111B1Z5" }) },
    ];

    expect(distinctInvoices(rows)).toHaveLength(2);
  });
});
