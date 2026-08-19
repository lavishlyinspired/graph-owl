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
    () => (data ? visibleRows(data.rows, filter, ruleFilter) : []),
    [data, filter, ruleFilter],
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
                  <td className="whitespace-nowrap px-3 py-2.5">
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
  const clickable = outcome.status === "flagged";

  const right =
    outcome.status === "flagged"
      ? `${outcome.found} finding${outcome.found === 1 ? "" : "s"}`
      : outcome.status === "passed"
        ? "checked, clean"
        : missing
          ? `no ${missing} in this period`
          : "could not run";

  return (
    <div
      className={`grid grid-cols-[1.25fr_150px_110px] items-baseline gap-3 rounded py-[4px] ${
        active ? "bg-white/70" : ""
      }`}
    >
      <div>
        <span className="text-[12.5px] text-reco-t1">
          {outcome.title ?? outcome.label}
          <WhyPopover
            title={outcome.title ?? outcome.label}
            explanation={{
              meaning: outcome.meaning ?? outcome.summary,
              next_action: outcome.next_action,
            }}
          />
        </span>
        {/* The rule's own identifier, kept: a CA defending a position needs it,
            and so does anyone reading a log. */}
        <div className="font-mono text-[9.5px] text-reco-t5">{outcome.label}</div>
        {/* The rule's own words. A label alone tells a reviewer nothing about
            what was or was not checked. */}
        {outcome.summary && (
          <div className="mt-[1px] text-[11px] leading-snug text-reco-t4">{outcome.summary}</div>
        )}
      </div>
      <span className="font-mono text-[10.5px] text-reco-t5">{outcome.governed_by ?? ""}</span>
      {clickable ? (
        <button
          type="button"
          onClick={() => onPick(outcome.label)}
          className="text-left text-[11px] underline decoration-dotted underline-offset-2"
          style={{ color: colour }}
          title={active ? "Show all invoices" : "Show the invoices behind this"}
        >
          {right} {active ? "▾" : "›"}
        </button>
      ) : (
        <span className="text-[11px]" style={{ color: colour }}>
          {right}
        </span>
      )}
    </div>
  );
}
