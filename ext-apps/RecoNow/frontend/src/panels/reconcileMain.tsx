import { useEffect, useMemo, useState } from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import {
  fetchReconciliation,
  type Bucket,
  type ReconRow,
  type Reconciliation,
  type RuleOutcome,
} from "../lib/api";
import { formatRupees } from "../lib/format";
import { WhyPopover } from "../components/WhyPopover";
import { DetailDrawer } from "../components/DetailDrawer";
import { ExplainCase } from "../components/ExplainCase";
import type { FigureExplanation } from "../lib/api";
import { visibleRows } from "../lib/rows";
import type { WorkspaceState } from "../lib/workspace";

/** The reconciliation result — what a reviewer looks at first.
 *
 *  Reco Now only ever showed exceptions, which cannot answer "how much of this
 *  period is done". The four buckets partition every invoice seen on either
 *  side, and the ITC position separates credit that is *deferred* from credit
 *  that is *lost* — a distinction the product previously collapsed into one
 *  "at risk" number. */

const BUCKET_META: Record<Bucket, { label: string; colour: string; hint: string }> = {
  matched: { label: "Matched", colour: "#2f6b4d", hint: "both sides agree" },
  review: { label: "Review", colour: "#a86a2c", hint: "both sides, values differ" },
  only_books: { label: "Only books", colour: "#a13f28", hint: "supplier has not filed" },
  only_portal: { label: "Only portal", colour: "#41508f", hint: "not recorded in books" },
};

const BUCKET_ORDER: readonly Bucket[] = ["matched", "review", "only_books", "only_portal"];

export default function ReconcileRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [data, setData] = useState<Reconciliation | null>(null);
  const [failed, setFailed] = useState(false);
  const [filter, setFilter] = useState<Bucket | null>(null);
  // Filtering the invoice table by rule is how "2 findings" stops being a
  // dead number: a reviewer clicks it and sees which two.
  const [ruleFilter, setRuleFilter] = useState<string | null>(null);
  // **The drawer replaces "filter a table the reader must then scroll to".**
  // A click whose result is off-screen reads as a click that did nothing.
  const [drawerRule, setDrawerRule] = useState<string | null>(null);
  // A row's own drawer: everything about *this invoice*, including the model's
  // reading of it. The rule drawer answers "what did this check find"; this
  // answers "what is going on with this invoice", and they are different
  // questions a reviewer asks at different moments.
  const [drawerRow, setDrawerRow] = useState<ReconRow | null>(null);
  const [search, setSearch] = useState("");
  const navigate = useNavigate();

  useEffect(() => {
    if (!clientId || !periodId) return;
    let cancelled = false;
    setFailed(false);
    fetchReconciliation(clientId, periodId)
      .then((d) => !cancelled && setData(d))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [clientId, periodId]);

  // One lookup for the whole table, built from the same guidance the rule
  // panel uses — two places rendering one rule differently is worse than
  // neither rendering it well.
  const titles = useMemo(
    () =>
      Object.fromEntries(
        (data?.rule_outcomes ?? []).map((o) => [o.label, o.title ?? o.label]),
      ) as Record<string, string>,
    [data],
  );

  const visible = useMemo(
    () => {
      const rows = data ? visibleRows(data.rows, filter, ruleFilter) : [];
      const needle = search.trim().toLowerCase();
      if (!needle) return rows;
      // GSTIN, supplier or invoice number — the three things a reviewer has
      // in hand when they come looking for one row.
      return rows.filter(
        (r) =>
          (r.invoice_no ?? "").toLowerCase().includes(needle) ||
          (r.supplier_name ?? "").toLowerCase().includes(needle) ||
          (r.supplier_gstin ?? "").toLowerCase().includes(needle),
      );
    },
    [data, filter, ruleFilter, search],
  );

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }
  if (failed) {
    return <div className="p-8 text-[13px] text-reco-bad">Could not load the reconciliation.</div>;
  }
  if (!data) {
    return <div className="p-8 text-[13px] text-reco-t4">Loading…</div>;
  }

  if (!data.have_books || !data.have_portal) {
    return (
      <div className="p-6">
        <Header rate={0} total={0} />
        <div className="rounded-[10px] border border-reco-line bg-white px-5 py-10 text-center">
          <div className="text-[13px] text-reco-t2">
            A reconciliation needs both sides.
          </div>
          <div className="mt-1 text-[12px] text-reco-t4">
            {data.have_books ? "GSTR-2B is missing." : "The purchase register is missing."}
          </div>
          <button
            type="button"
            onClick={() => navigate("/pipeline")}
            className="mt-4 rounded-[7px] bg-reco-t0 px-3.5 py-[7px] text-[12.5px] font-semibold text-white"
          >
            Go to Upload &amp; map
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="p-6 pb-11">
      <Header rate={data.match_rate} total={data.total} />

      {/* The two numbers a reviewer opens this screen for, before the counts.
          Taken from the delivered mockup, which leads with them for the same
          reason: "7 matched" is a fact about rows, and these are facts about
          money. */}
      <div className="mb-3.5 grid gap-3 sm:grid-cols-2">
        <HeadlineCard
          tone="good"
          label="ITC confirmed safe"
          amount={data.itc.confirmed}
          hint={`${data.counts.matched} matched — claim with confidence`}
          explanation={data.explain_itc?.confirmed}
        />
        <HeadlineCard
          tone="bad"
          label="ITC at risk"
          amount={data.itc.blocked + data.itc.under_review}
          hint="blocked outright, plus the part still in dispute"
          explanation={data.explain_itc?.blocked}
        />
      </div>

      <div className="mb-3.5 grid grid-cols-4 gap-3">
        {BUCKET_ORDER.map((bucket) => {
          const meta = BUCKET_META[bucket];
          const active = filter === bucket;
          return (
            <button
              key={bucket}
              type="button"
              onClick={() => setFilter(active ? null : bucket)}
              className={`rounded-[10px] border bg-white p-4 text-left transition-colors ${
                active ? "border-reco-t0" : "border-reco-line hover:border-reco-line-3"
              }`}
            >
              <div className="mb-2 flex items-center font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                {meta.label.toUpperCase()}
                {/* Every figure says how it was worked out and what to do — the
                    explanation travels with the data rather than living in this
                    component, so one number cannot be explained two ways on two
                    screens. */}
                <WhyPopover title={meta.label} explanation={data.explain?.[bucket]} />
              </div>
              <div className="font-mono text-[26px]" style={{ color: meta.colour }}>
                {data.counts[bucket]}
              </div>
              <div className="mt-1.5 text-[11.5px] text-reco-t4">{meta.hint}</div>
            </button>
          );
        })}
      </div>

      {/* Search and export, from the delivered mockup. A reviewer who already
          knows which invoice they want should not have to scan for it, and a
          working paper that cannot leave the screen is not a working paper. */}
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <input
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search by GSTIN, supplier or invoice number…"
          aria-label="Search invoices"
          className="min-w-[240px] flex-1 rounded border border-reco-line bg-white px-3 py-1.5 text-[12.5px] text-reco-t1 placeholder:text-reco-t5"
        />
        <button
          type="button"
          onClick={() => exportCsv(visible)}
          className="rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
        >
          Export CSV
        </button>
        <a
          href={`/workingpaper`}
          className="rounded border border-reco-line px-3 py-1.5 text-[12px] text-reco-t2 hover:border-reco-accent hover:text-reco-accent"
        >
          Working paper
        </a>
      </div>

      {/* **The table comes before the checks now.** It used to sit below the
          statutory-check blocks, the ITC panel and the ladder — so clicking
          "2 findings" filtered a table the reader then had to scroll to find.
          A filter whose result is off-screen reads as a filter that did
          nothing. */}
      <div className="mb-3.5 overflow-hidden rounded-[10px] border border-reco-line bg-white">
        <div className="flex items-center justify-between border-b border-reco-line px-[18px] py-2.5">
          <span className="text-[13px] font-semibold text-reco-t1">
            {ruleFilter ?? (filter ? BUCKET_META[filter].label : "All invoices")}
            <span className="ml-2 font-mono text-[11px] font-normal text-reco-t5">
              {visible.length}
            </span>
          </span>
          {(filter || ruleFilter) && (
            <button
              type="button"
              onClick={() => {
                setFilter(null);
                setRuleFilter(null);
              }}
              className="text-[12px] text-reco-accent"
            >
              Show all
            </button>
          )}
        </div>
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-left">
            <thead>
              <tr className="border-b border-reco-line bg-reco-panel-2 font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">
                <th className="px-3 py-2.5">INVOICE</th>
                <th className="px-3 py-2.5">SUPPLIER</th>
                <th className="px-3 py-2.5 text-right">BOOKS</th>
                <th className="px-3 py-2.5 text-right">PORTAL</th>
                <th className="px-3 py-2.5 text-right">DIFF</th>
                <th className="px-3 py-2.5">STATE</th>
                <th className="px-3 py-2.5">REASON</th>
              </tr>
            </thead>
            <tbody>
              {visible.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-3 py-9 text-center text-[12.5px] text-reco-t4">
                    Nothing in this bucket.
                  </td>
                </tr>
              )}
              {visible.map((row, i) => (
                <tr
                  key={`${row.supplier_gstin}-${row.invoice_no}-${i}`}
                  className="border-b border-reco-row last:border-b-0 hover:bg-reco-panel-2"
                >
                  <td className="whitespace-nowrap px-3 py-2.5 font-mono text-[11.5px] text-reco-t1">
                    {row.invoice_no ?? "—"}
                  </td>
                  <td className="px-3 py-2.5 text-[12px] text-reco-t2">
                    {row.supplier_name ?? "—"}
                    <div className="font-mono text-[10px] text-reco-t5">{row.supplier_gstin}</div>
                  </td>
                  <td className="whitespace-nowrap px-3 py-2.5 text-right font-mono text-[11.5px] text-reco-t1">
                    {row.bucket === "only_portal" ? "—" : formatRupees(row.books_taxable)}
                  </td>
                  <td className="whitespace-nowrap px-3 py-2.5 text-right font-mono text-[11.5px] text-reco-t1">
                    {row.bucket === "only_books" ? "—" : formatRupees(row.portal_taxable)}
                  </td>
                  <td
                    className="whitespace-nowrap px-3 py-2.5 text-right font-mono text-[11.5px]"
                    style={{ color: row.difference === 0 ? "#8a857c" : "#a13f28" }}
                  >
                    {row.bucket === "matched" || row.bucket === "review"
                      ? formatRupees(Math.abs(row.difference))
                      : "—"}
                  </td>
                  <td
                    className="cursor-pointer whitespace-nowrap px-3 py-2.5"
                    onClick={() => setDrawerRow(row)}
                  >
                    <span
                      className="rounded border px-1.5 py-0.5 font-mono text-[9.5px]"
                      style={{
                        color: BUCKET_META[row.bucket].colour,
                        borderColor: BUCKET_META[row.bucket].colour + "55",
                      }}
                    >
                      {BUCKET_META[row.bucket].label.toUpperCase()}
                    </span>
                  </td>
                  <td className="px-3 py-2.5 font-mono text-[10.5px] text-reco-t4">
                    {/* Titles, not IRIs. `gst:PaymentOverdue` is a label a
                        rule author chose; the reader needs to know what is
                        wrong. Falls back to the IRI only where the pack has
                        authored no title. */}
                    {row.labels.length > 0
                      ? row.labels.map((l) => titles[l] ?? l).join(", ")
                      : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <RulePanel
        outcomes={data.rule_outcomes}
        unsupported={data.checks_disabled}
        active={ruleFilter}
        onPick={(label) => setDrawerRule(label)}
      />

      {/* The ITC position lives on its own screen, which shows the same five
          classes from the same computation. Two screens rendering one figure
          is two places to keep in step, and the headline cards above already
          carry the two numbers this screen needs. */}
      <Ladder rows={visible} />

      <DetailDrawer
        open={drawerRow !== null}
        title={drawerRow?.invoice_no ?? ""}
        subtitle={drawerRow?.supplier_name ?? undefined}
        onClose={() => setDrawerRow(null)}
      >
        {drawerRow && (
          <RowDrawerBody row={drawerRow} titles={titles} clientId={clientId} periodId={periodId} />
        )}
      </DetailDrawer>

      <DetailDrawer
        open={drawerRule !== null}
        title={
          drawerRule
            ? (data.rule_outcomes.find((o) => o.label === drawerRule)?.title ?? drawerRule)
            : ""
        }
        subtitle={drawerRule ?? undefined}
        onClose={() => setDrawerRule(null)}
      >
        <RuleDrawerBody
          rule={drawerRule}
          outcome={data.rule_outcomes.find((o) => o.label === drawerRule)}
          rows={data.rows.filter((r) => drawerRule && r.labels.includes(drawerRule))}
        />
      </DetailDrawer>
    </div>
  );
}

/** One of the two headline figures. Large, because they are the answer; the
 *  bucket counts below are how the answer was arrived at. */
function HeadlineCard({
  tone,
  label,
  amount,
  hint,
  explanation,
}: {
  readonly tone: "good" | "bad";
  readonly label: string;
  readonly amount: number;
  readonly hint: string;
  readonly explanation: FigureExplanation | undefined;
}) {
  const good = tone === "good";
  return (
    <div
      className={`rounded-[10px] border p-4 ${
        good ? "border-emerald-200 bg-emerald-50/60" : "border-red-200 bg-red-50/50"
      }`}
    >
      <div className="mb-1.5 flex items-center font-mono text-[9.5px] tracking-[0.12em]">
        <span className={good ? "text-emerald-800" : "text-red-800"}>
          {label.toUpperCase()}
        </span>
        <WhyPopover title={label} explanation={explanation} />
      </div>
      <div
        className={`font-mono text-[30px] leading-none ${
          good ? "text-emerald-700" : "text-red-700"
        }`}
      >
        {formatRupees(amount)}
      </div>
      <div className="mt-2 text-[11.5px] text-reco-t4">{hint}</div>
    </div>
  );
}

/** Everything about one invoice, including the model's reading of it.
 *
 *  Reached by clicking the row's state or reason — the two cells a reader
 *  looks at when they want to know *why* a row is where it is. */
function RowDrawerBody({
  row,
  titles,
  clientId,
  periodId,
}: {
  readonly row: ReconRow;
  readonly titles: Record<string, string>;
  readonly clientId: string;
  readonly periodId: string;
}) {
  return (
    <div className="space-y-4">
      <div>
        <span
          className="inline-block rounded border px-1.5 py-[1px] font-mono text-[10px] uppercase"
          style={{
            color: BUCKET_META[row.bucket].colour,
            borderColor: BUCKET_META[row.bucket].colour + "55",
          }}
        >
          {BUCKET_META[row.bucket].label}
        </span>
        <span className="ml-2 text-[11.5px] text-reco-t4">{BUCKET_META[row.bucket].hint}</span>
      </div>

      <dl className="grid grid-cols-2 gap-x-3 gap-y-2 text-[12px]">
        <Amount label="Books taxable" value={row.books_taxable} />
        <Amount label="Portal taxable" value={row.portal_taxable} />
        <Amount label="Books tax" value={row.books_tax} />
        <Amount label="Portal tax" value={row.portal_tax} />
        {row.difference !== 0 && (
          <Amount label="Difference" value={Math.abs(row.difference)} tone="bad" />
        )}
      </dl>

      {row.labels.length > 0 && (
        <div>
          <div className="mb-1.5 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
            What was found
          </div>
          {row.labels.map((label) => (
            <div key={label} className="border-b border-reco-row py-1.5 last:border-b-0">
              <div className="text-[12px] text-reco-t1">{titles[label] ?? label}</div>
              <div className="font-mono text-[9.5px] text-reco-t5">{label}</div>
            </div>
          ))}
        </div>
      )}

      {/* The model reads this whole row — both sides, tax heads, dates — and
          says what is notable. Grounded, so a figure your data does not carry
          is refused and the computed sentence shown instead. */}
      {clientId && periodId && row.case_id && (
        <ExplainCase clientId={clientId} periodId={periodId} caseId={row.case_id} />
      )}
      {!row.case_id && (
        <p className="text-[11.5px] text-reco-t4">
          Nothing was flagged on this invoice, so there is no case to explain.
        </p>
      )}
    </div>
  );
}

function Amount({
  label,
  value,
  tone,
}: {
  readonly label: string;
  readonly value: number | null | undefined;
  readonly tone?: "bad";
}) {
  return (
    <div>
      <dt className="font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">{label}</dt>
      <dd className={`font-mono text-[12.5px] ${tone === "bad" ? "text-reco-bad" : "text-reco-t1"}`}>
        {value == null ? "—" : formatRupees(value)}
      </dd>
    </div>
  );
}

/** What one rule found, in the drawer: why it fired, what to do, and every
 *  invoice it named — without leaving the screen. */
function RuleDrawerBody({
  rule,
  outcome,
  rows,
}: {
  readonly rule: string | null;
  readonly outcome: RuleOutcome | undefined;
  readonly rows: readonly ReconRow[];
}) {
  if (!rule) return null;

  return (
    <>
      {outcome?.meaning && (
        <p className="mb-3 text-[12.5px] leading-relaxed text-reco-t2">{outcome.meaning}</p>
      )}
      {outcome?.next_action && (
        <div className="mb-4 rounded border border-reco-line bg-reco-panel-2 p-3">
          <div className="mb-1 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
            What to do
          </div>
          <p className="text-[12px] leading-relaxed text-reco-t2">{outcome.next_action}</p>
        </div>
      )}
      {outcome?.governed_by && (
        <div className="mb-4 inline-block rounded border border-reco-line px-2 py-1 font-mono text-[10.5px] text-reco-t4">
          {outcome.governed_by}
        </div>
      )}

      <div className="mb-2 font-mono text-[9.5px] uppercase tracking-wider text-reco-t5">
        {rows.length} invoice{rows.length === 1 ? "" : "s"}
      </div>
      {rows.map((row, i) => (
        <div
          key={`${row.invoice_no}-${i}`}
          className="border-b border-reco-row py-2.5 last:border-b-0"
        >
          <div className="flex items-baseline justify-between gap-2">
            <span className="font-mono text-[11.5px] text-reco-t1">{row.invoice_no}</span>
            <span className="font-mono text-[11.5px] text-reco-t2">
              {formatRupees(row.books_tax ?? 0)}
            </span>
          </div>
          <div className="text-[11px] text-reco-t4">{row.supplier_name ?? "—"}</div>
          {row.difference !== 0 && (
            <div className="mt-0.5 font-mono text-[10.5px] text-reco-bad">
              differs by {formatRupees(Math.abs(row.difference))}
            </div>
          )}
        </div>
      ))}
    </>
  );
}

/** The filtered rows as CSV.
 *
 *  **What is on screen, not the whole period.** A reviewer who has filtered to
 *  one rule and exports expects that filter to hold; an export that silently
 *  widened would be a different document from the one they were looking at. */
function exportCsv(rows: readonly ReconRow[]) {
  const header = [
    "Invoice", "Supplier", "GSTIN", "Bucket",
    "Books taxable", "Portal taxable", "Books tax", "Portal tax", "Difference", "Findings",
  ];
  const escape = (value: string) => `"${value.replace(/"/g, '""')}"`;
  const body = rows.map((r) =>
    [
      r.invoice_no ?? "", r.supplier_name ?? "", r.supplier_gstin ?? "", r.bucket,
      r.books_taxable, r.portal_taxable, r.books_tax, r.portal_tax, r.difference,
      r.labels.join("; "),
    ]
      .map((cell) => escape(String(cell)))
      .join(","),
  );
  const blob = new Blob([[header.map(escape).join(","), ...body].join("\n")], {
    type: "text/csv;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "reconciliation.csv";
  link.click();
  URL.revokeObjectURL(url);
}

function Header({ rate, total }: { readonly rate: number; readonly total: number }) {
  return (
    <div className="mb-4 flex items-end justify-between">
      <div>
        <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">Reconcile</h1>
        <p className="text-[12.5px] text-reco-t4">
          Every invoice on either side, in exactly one state.
        </p>
      </div>
      {total > 0 && (
        <div className="text-right">
          <div className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">MATCH RATE</div>
          <div className="font-mono text-[24px] text-reco-t1">{(rate * 100).toFixed(1)}%</div>
          <div className="text-[11px] text-reco-t5">{total} invoices</div>
        </div>
      )}
    </div>
  );
}

/** The books↔portal ladder. One rung per invoice: a line drawn between the
 *  two sides when both have it, a lone dot when only one does. It conveys the
 *  shape of a reconciliation in a way a table cannot. */
function Ladder({ rows }: { readonly rows: readonly ReconRow[] }) {
  const shown = rows.slice(0, 14);
  if (shown.length === 0) return null;

  return (
    <div className="mb-3.5 rounded-[10px] border border-reco-line bg-white p-4">
      <div className="mb-3.5 flex items-baseline justify-between">
        <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
          BOOKS &nbsp;—&nbsp; PORTAL
        </span>
        <span className="text-[11.5px] text-reco-t5">
          {rows.length > shown.length ? `first ${shown.length} of ${rows.length}` : "a line means both sides carry it"}
        </span>
      </div>
      <div className="flex flex-col gap-1.5">
        {shown.map((row, i) => {
          const colour = BUCKET_META[row.bucket].colour;
          const hasBooks = row.bucket !== "only_portal";
          const hasPortal = row.bucket !== "only_books";
          return (
            <div
              key={`${row.invoice_no}-${i}`}
              className="grid grid-cols-[1fr_18px_120px_18px_1fr] items-center gap-2"
            >
              <span className="truncate text-right font-mono text-[11px] text-reco-t2">
                {hasBooks ? `${formatRupees(row.books_taxable)}  ${row.invoice_no}` : ""}
              </span>
              <span
                className="h-[7px] w-[7px] justify-self-center rounded-full"
                style={{ background: hasBooks ? colour : "transparent" }}
              />
              <span
                className="h-[2px] w-full"
                style={{ background: hasBooks && hasPortal ? colour : "transparent" }}
              />
              <span
                className="h-[7px] w-[7px] justify-self-center rounded-full"
                style={{ background: hasPortal ? colour : "transparent" }}
              />
              <span className="truncate font-mono text-[11px] text-reco-t2">
                {hasPortal ? `${row.invoice_no}  ${formatRupees(row.portal_taxable)}` : ""}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RulePanel({
  outcomes,
  unsupported,
  active,
  onPick,
}: {
  readonly outcomes: readonly RuleOutcome[];
  readonly unsupported: Record<string, string>;
  readonly active: string | null;
  readonly onPick: (label: string) => void;
}) {
  // Before any reconciliation has run there are no outcomes, but a reviewer
  // still needs to know which checks the uploaded files can support.
  if (outcomes.length === 0) {
    const pending = Object.entries(unsupported);
    if (pending.length === 0) return null;
    return (
      <section className="mb-3.5 overflow-hidden rounded-[10px] border-2 border-reco-amber-border bg-reco-amber-bg">
        <div className="border-b border-reco-amber-border px-4 py-2.5">
          <span className="font-mono text-[10px] font-semibold tracking-[0.1em] text-reco-amber">
            ⚠ NOT YET RECONCILED
          </span>
          <span className="ml-2 text-[11.5px] text-reco-t2">
            {pending.length} check{pending.length === 1 ? "" : "s"} cannot run on the files uploaded
          </span>
        </div>
        <div className="px-4 py-2.5">
          {pending.map(([label, reason]) => (
            <div key={label} className="grid grid-cols-[1.3fr_2fr] gap-3 py-[3px]">
              <span className="font-mono text-[11.5px] text-reco-t1">{label}</span>
              <span className="text-[11.5px] text-reco-t2">{reason}</span>
            </div>
          ))}
        </div>
      </section>
    );
  }

  const flagged = outcomes.filter((o) => o.status === "flagged");
  const notEvaluated = outcomes.filter((o) => o.status === "notEvaluated");
  const passed = outcomes.filter((o) => o.status === "passed");

  return (
    <section className="mb-3.5">
      <div className="mb-2 flex items-baseline justify-between">
        <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
          STATUTORY CHECKS
        </span>
        <div className="flex items-center gap-2">
          <Chip n={flagged.length} label="failed" colour="#a13f28" />
          <Chip n={notEvaluated.length} label="not evaluated" colour="#a86a2c" />
          <Chip n={passed.length} label="passed" colour="#2f6b4d" />
        </div>
      </div>

      {/* First, and boxed, because it is the state that silently reads as good
          news. A reviewer who skims must still see it. */}
      {notEvaluated.length > 0 && (
        <StateBlock
          tone="amber"
          active={active}
          onPick={onPick}
          heading="⚠ NOT EVALUATED"
          note="These were not checked. That is not the same as passing."
          outcomes={notEvaluated}
        />
      )}
      {flagged.length > 0 && (
        <StateBlock
          tone="red"
          active={active}
          onPick={onPick}
          heading="✕ FAILED"
          note="Ran, and found something to answer for."
          outcomes={flagged}
        />
      )}
      {passed.length > 0 && (
        <StateBlock
          tone="green"
          active={active}
          onPick={onPick}
          heading="✓ PASSED"
          note="Ran against this period's data and found nothing."
          outcomes={passed}
        />
      )}
    </section>
  );
}

function Chip({
  n,
  label,
  colour,
}: {
  readonly n: number;
  readonly label: string;
  readonly colour: string;
}) {
  const shade = n === 0 ? "#8a857c" : colour;
  return (
    <span
      className="rounded-full border px-2 py-[2px] font-mono text-[10px]"
      style={{ color: shade, borderColor: shade + "55" }}
    >
      {n} {label}
    </span>
  );
}

const TONE = {
  amber: { border: "#f0dcc2", bg: "#fdf3e7", text: "#a86a2c" },
  red: { border: "#eed7d1", bg: "#fdf1ee", text: "#a13f28" },
  green: { border: "#e3e0d9", bg: "#ffffff", text: "#2f6b4d" },
} as const;

function StateBlock({
  tone,
  heading,
  note,
  outcomes,
  active,
  onPick,
}: {
  readonly tone: keyof typeof TONE;
  readonly heading: string;
  readonly note: string;
  readonly outcomes: readonly RuleOutcome[];
  readonly active: string | null;
  readonly onPick: (label: string) => void;
}) {
  const t = TONE[tone];
  return (
    <div
      className="mb-2 overflow-hidden rounded-[10px]"
      style={{
        background: t.bg,
        border: `${tone === "amber" ? 2 : 1}px solid ${t.border}`,
      }}
    >
      <div
        className="flex items-baseline gap-2.5 px-4 py-2"
        style={{ borderBottom: `1px solid ${t.border}` }}
      >
        <span
          className="font-mono text-[10px] font-semibold tracking-[0.1em]"
          style={{ color: t.text }}
        >
          {heading}
        </span>
        <span className="text-[11px] text-reco-t4">{note}</span>
      </div>
      <div className="px-4 py-1.5">
        {outcomes.map((o) => (
          <RuleLine
            key={o.label}
            outcome={o}
            colour={t.text}
            active={active === o.label}
            onPick={onPick}
          />
        ))}
      </div>
    </div>
  );
}

/** One rule, on one line — the delivered mockup's "Mismatch Classification"
 *  shape: dot, title, count, the citation as a chip, the action inline, and
 *  the money on the right.
 *
 *  **It replaces three stacked lines per rule.** Twenty-one rules at three
 *  lines each is a page of scrolling before the reader reaches anything they
 *  can act on, and the thing they came for — how much and what to do — was the
 *  part pushed furthest down. */
function RuleLine({
  outcome,
  colour,
  active,
  onPick,
}: {
  readonly outcome: RuleOutcome;
  readonly colour: string;
  readonly active: boolean;
  readonly onPick: (label: string) => void;
}) {
  const missing = outcome.unmet.map((u) => u.split("#").pop()).join(", ");
  const flagged = outcome.status === "flagged";

  const right = flagged
    ? `${outcome.found} finding${outcome.found === 1 ? "" : "s"}`
    : outcome.status === "passed"
      ? "checked, clean"
      : missing
        ? `no ${missing} in this period`
        : "could not run";

  const body = (
    <>
      <span className="mt-[6px] h-[6px] w-[6px] shrink-0 rounded-full" style={{ background: colour }} />

      <span className="min-w-0 flex-1">
        <span className="text-[12.5px] text-reco-t1">{outcome.title ?? outcome.label}</span>
        {/* The rule's own identifier stays reachable — a CA defending a
            position needs it — without taking a line of its own. */}
        <WhyPopover
          title={outcome.title ?? outcome.label}
          explanation={{
            meaning: outcome.meaning ?? outcome.summary,
            next_action: outcome.next_action,
          }}
        />
        {outcome.next_action && (
          <span className="ml-2 text-[11.5px] text-reco-t4">{outcome.next_action}</span>
        )}
      </span>

      {outcome.governed_by && (
        <span className="shrink-0 rounded border border-reco-line px-1.5 py-[1px] font-mono text-[10px] text-reco-t5">
          {outcome.governed_by}
        </span>
      )}

      <span
        className="w-[110px] shrink-0 text-right text-[11.5px]"
        style={{ color: flagged ? colour : undefined }}
      >
        {right}
        {flagged && " ›"}
      </span>
    </>
  );

  const className = `flex w-full items-start gap-2 rounded px-1 py-[5px] text-left ${
    active ? "bg-white/70" : ""
  } ${flagged ? "hover:bg-white/60" : ""}`;

  return flagged ? (
    <button type="button" onClick={() => onPick(outcome.label)} className={className}>
      {body}
    </button>
  ) : (
    <div className={className}>{body}</div>
  );
}
