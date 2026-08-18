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

  const visible = useMemo(
    () => (data ? (filter ? data.rows.filter((r) => r.bucket === filter) : data.rows) : []),
    [data, filter],
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
              <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                {meta.label.toUpperCase()}
              </div>
              <div className="font-mono text-[26px]" style={{ color: meta.colour }}>
                {data.counts[bucket]}
              </div>
              <div className="mt-1.5 text-[11.5px] text-reco-t4">{meta.hint}</div>
            </button>
          );
        })}
      </div>

      <RulePanel outcomes={data.rule_outcomes} unsupported={data.checks_disabled} />

      <ItcPositionPanel itc={data.itc} />

      <Ladder rows={visible} />

      <div className="mt-3.5 overflow-hidden rounded-[10px] border border-reco-line bg-white">
        <div className="flex items-center justify-between border-b border-reco-line px-[18px] py-2.5">
          <span className="text-[13px] font-semibold text-reco-t1">
            {filter ? BUCKET_META[filter].label : "All invoices"}
            <span className="ml-2 font-mono text-[11px] font-normal text-reco-t5">
              {visible.length}
            </span>
          </span>
          {filter && (
            <button
              type="button"
              onClick={() => setFilter(null)}
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
                    {row.labels.length > 0 ? row.labels.join(", ") : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
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

/** Where the credit actually stands. The `pending` vs `blocked` split is the
 *  point: one is deferred and recoverable by chasing the supplier, the other
 *  is gone whatever anyone does. */
function ItcPositionPanel({ itc }: { readonly itc: Reconciliation["itc"] }) {
  const classes = [
    { key: "confirmed", label: "CONFIRMED", colour: "#2f6b4d", hint: "matched — claim it" },
    { key: "pending", label: "PENDING", colour: "#a86a2c", hint: "deferred until the supplier files" },
    { key: "under_review", label: "UNDER REVIEW", colour: "#c9803a", hint: "the disagreement only" },
    { key: "blocked", label: "BLOCKED", colour: "#a13f28", hint: "s.17(5) — lost" },
    { key: "unclaimed", label: "UNCLAIMED", colour: "#41508f", hint: "on the portal, not in books" },
  ] as const;

  return (
    <div className="mb-3.5 rounded-[10px] border border-reco-line bg-white p-4">
      <div className="mb-3 flex items-baseline justify-between">
        <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">ITC POSITION</span>
        <span className="text-[11.5px] text-reco-t5">
          pending is deferred, not lost — blocked is lost
        </span>
      </div>
      <div className="grid grid-cols-5 gap-3">
        {classes.map((c) => (
          <div key={c.key}>
            <div className="font-mono text-[9px] tracking-[0.1em] text-reco-t5">{c.label}</div>
            <div className="mt-1 font-mono text-[17px]" style={{ color: c.colour }}>
              {formatRupees(itc[c.key])}
            </div>
            <div className="mt-0.5 text-[10.5px] leading-snug text-reco-t5">{c.hint}</div>
          </div>
        ))}
      </div>
    </div>
  );
}


/** Every rule, in one of three states.
 *
 *  A check that never ran and a check that found nothing are opposite claims
 *  and used to render identically as "no issues". For a statutory test —
 *  Rule 37, s.16(2)(b), s.17(5) — that difference is a client's money.
 *
 *  So the three states are **separate blocks with their own headings**, not
 *  one list distinguished by the colour of a tick. A reviewer skimming this
 *  panel should be unable to mistake "not evaluated" for "passed" without
 *  reading the marks, and Not evaluated comes first because it is the state
 *  that silently reads as good news.
 *
 *  The states come from **graph-owl's own execution record**, not from
 *  inspecting which files were uploaded. The engine probes each rule's
 *  declared requirements before running it and reports what it found; this
 *  renders that. "Could not evaluate, and here is what was missing" is
 *  evidence about the run, and it belongs in the engine.
 */
function RulePanel({
  outcomes,
  unsupported,
}: {
  readonly outcomes: readonly RuleOutcome[];
  readonly unsupported: Record<string, string>;
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
          heading="⚠ NOT EVALUATED"
          note="These were not checked. That is not the same as passing."
          outcomes={notEvaluated}
        />
      )}
      {flagged.length > 0 && (
        <StateBlock
          tone="red"
          heading="✕ FAILED"
          note="Ran, and found something to answer for."
          outcomes={flagged}
        />
      )}
      {passed.length > 0 && (
        <StateBlock
          tone="green"
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
}: {
  readonly tone: keyof typeof TONE;
  readonly heading: string;
  readonly note: string;
  readonly outcomes: readonly RuleOutcome[];
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
          <RuleLine key={o.label} outcome={o} colour={t.text} />
        ))}
      </div>
    </div>
  );
}

function RuleLine({
  outcome,
  colour,
}: {
  readonly outcome: RuleOutcome;
  readonly colour: string;
}) {
  const missing = outcome.unmet.map((u) => u.split("#").pop()).join(", ");
  return (
    <div className="grid grid-cols-[1.3fr_0.9fr_1fr] items-baseline gap-3 py-[3px]">
      <span className="font-mono text-[11.5px] text-reco-t1">{outcome.label}</span>
      <span className="font-mono text-[10.5px] text-reco-t5">{outcome.governed_by ?? ""}</span>
      <span className="text-[11px]" style={{ color: colour }}>
        {outcome.status === "flagged"
          ? `${outcome.found} finding${outcome.found === 1 ? "" : "s"}`
          : outcome.status === "passed"
            ? "checked, clean"
            : missing
              ? `no ${missing} in this period`
              : "could not run"}
      </span>
    </div>
  );
}
