import { useEffect, useState } from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import { fetchDashboard, fetchExceptions, type Dashboard, type ExceptionGroup } from "../lib/api";
import { formatRupees } from "../lib/format";
import { strings } from "../lib/strings";
import type { WorkspaceState } from "../lib/workspace";

/** The close-readiness dashboard.
 *
 *  This screen previously rendered the delivered mockup's own figures as
 *  constants — ₹2.84 Cr reconciled, "9,842 invoices matched", a written
 *  briefing quoting "24 IMS records", a five-month match-rate trend, and
 *  agent token spend. None of it came from a query, and the two live numbers
 *  it did read fell back to `?? 38` and `?? 420000` when the fetch failed, so
 *  a broken backend produced a confident dashboard.
 *
 *  What is shown here is computed from the uploaded files and the
 *  reconciliation. Panels whose data this backend does not produce —
 *  period-over-period trend, entity-resolution counts, assistant spend — are
 *  absent rather than illustrated; see the footer note. */
export default function HomeRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [groups, setGroups] = useState<readonly ExceptionGroup[]>([]);
  const [failed, setFailed] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    if (!clientId || !periodId) return;
    let cancelled = false;
    setFailed(false);
    Promise.all([fetchDashboard(clientId, periodId), fetchExceptions(clientId, periodId)])
      .then(([d, g]) => {
        if (cancelled) return;
        setDashboard(d);
        setGroups(g);
      })
      .catch(() => {
        if (cancelled) return;
        setDashboard(null);
        setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [clientId, periodId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }
  if (failed) {
    return (
      <div className="p-8 text-[13px] text-reco-bad">
        Could not load this period from the backend.
      </div>
    );
  }
  if (!dashboard) {
    return <div className="p-8 text-[13px] text-reco-t4">Loading…</div>;
  }

  const hasData = dashboard.datasets.length > 0;

  return (
    <div className="p-6 pb-11">
      <div className="mb-4 flex items-end justify-between">
        <div>
          <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">
            {dashboard.period_label ? `${dashboard.period_label} close` : strings.dashboardTitle}
          </h1>
          <p className="text-[12.5px] text-reco-t4">
            {hasData
              ? `${dashboard.case_count} case${dashboard.case_count === 1 ? "" : "s"} across ${dashboard.invoice_count} invoice${dashboard.invoice_count === 1 ? "" : "s"} still need a decision.`
              : strings.dashboardEmpty}
          </p>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => navigate("/pipeline")}
            className="rounded-[7px] border border-reco-line-3 bg-white px-3.5 py-[7px] text-[12.5px] text-reco-t1 hover:border-reco-t5"
          >
            Upload &amp; map
          </button>
          <button
            type="button"
            onClick={() => navigate("/register")}
            className="rounded-[7px] bg-reco-t0 px-3.5 py-[7px] text-[12.5px] font-semibold text-white"
          >
            Open register
          </button>
        </div>
      </div>

      {!hasData ? (
        <div className="rounded-[10px] border border-reco-line bg-white px-5 py-10 text-center">
          <div className="text-[13px] text-reco-t2">Nothing uploaded for this period yet.</div>
          <div className="mt-1 text-[12px] text-reco-t4">
            Upload a purchase register and a GSTR-2B, confirm the mapping, and reconcile.
          </div>
          <button
            type="button"
            onClick={() => navigate("/pipeline")}
            className="mt-4 rounded-[7px] bg-reco-t0 px-3.5 py-[7px] text-[12.5px] font-semibold text-white"
          >
            Go to Upload &amp; map
          </button>
        </div>
      ) : (
        <>
          <div className="mb-3.5 grid grid-cols-3 gap-3">
            <Card
              label="BOOKS · NOT FLAGGED"
              value={dashboard.clean_total === null ? "—" : formatRupees(dashboard.clean_total)}
              sub={
                dashboard.books_total === null
                  ? "no taxable column mapped"
                  : `of ${formatRupees(dashboard.books_total)} booked this period`
              }
              color="#2f6b4d"
              border="#e3e0d9"
            />
            <Card
              label="NEEDS REVIEW"
              value={String(dashboard.case_count)}
              sub={`across ${groups.length} reason code${groups.length === 1 ? "" : "s"}`}
              color="#a86a2c"
              border="#f0dcc2"
            />
            <Card
              label={strings.itcAtRiskLabel}
              value={formatRupees(dashboard.total_exposure)}
              sub={`${dashboard.invoice_count} invoice${dashboard.invoice_count === 1 ? "" : "s"} · ${dashboard.supplier_count} supplier${dashboard.supplier_count === 1 ? "" : "s"} to chase`}
              color="#a13f28"
              border="#eed7d1"
            />
          </div>

          <div className="grid grid-cols-[1fr_320px] items-start gap-3.5">
            <div className="overflow-hidden rounded-[10px] border border-reco-line bg-white">
              <div className="flex items-center justify-between border-b border-reco-line px-[18px] py-2.5">
                <span className="text-[13px] font-semibold text-reco-t1">
                  {strings.dashboardNeedsDecision}
                </span>
                <span className="font-mono text-[10px] text-reco-t5">sorted by ITC exposure</span>
              </div>
              {dashboard.needs_decision.length === 0 ? (
                <div className="px-[18px] py-9 text-center text-[12.5px] text-reco-t4">
                  Nothing is waiting on a decision in this period.
                </div>
              ) : (
                dashboard.needs_decision.map((c, i) => (
                  <button
                    key={`${c.invoice_no}-${c.reason_code ?? i}`}
                    type="button"
                    onClick={() => navigate(`/register?reason_code=${encodeURIComponent(c.reason_code ?? "")}`)}
                    className="flex w-full items-center gap-3 border-b border-reco-row px-[18px] py-3 text-left last:border-b-0 hover:bg-reco-panel-2"
                  >
                    <span className="h-[7px] w-[7px] flex-none rounded-[2px] bg-reco-bad" />
                    <div className="flex-1">
                      <div className="text-[12.5px] text-reco-t1">{c.reason_code ?? "unclassified"}</div>
                      <div className="mt-[2px] font-mono text-[10.5px] text-reco-t5">
                        {c.invoice_no}
                        {c.supplier_name ? ` · ${c.supplier_name}` : ""}
                      </div>
                    </div>
                    <div className="text-right">
                      <div className="font-mono text-[12.5px] text-reco-t1">
                        {formatRupees(c.exposure)}
                      </div>
                      <div className="font-mono text-[10px] text-reco-t5">{c.status}</div>
                    </div>
                  </button>
                ))
              )}
            </div>

            <div className="flex flex-col gap-3.5">
              <div className="rounded-[10px] border border-reco-line bg-white p-4">
                <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                  PERIOD STATE
                </div>
                {dashboard.datasets.map((d) => (
                  <div
                    key={d.kind}
                    className="flex items-center justify-between border-b border-reco-row py-[7px] last:border-b-0"
                  >
                    <span className="text-[12px] text-reco-t2">{d.kind}</span>
                    <span
                      className={`font-mono text-[10.5px] ${d.confirmed ? "text-reco-ok" : "text-reco-amber"}`}
                    >
                      {d.total_rows} rows · {d.confirmed ? "mapped" : "not confirmed"}
                    </span>
                  </div>
                ))}
                <div className="flex items-center justify-between border-t border-reco-line-2 pt-[9px]">
                  <span className="text-[12px] text-reco-t2">Awaiting approval</span>
                  <span className="font-mono text-[10.5px] text-reco-t4">
                    {dashboard.pending_approvals}
                  </span>
                </div>
              </div>

              {groups.length > 0 && (
                <div className="rounded-[10px] border border-reco-line bg-white p-4">
                  <div className="mb-2.5 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                    BY REASON CODE
                  </div>
                  {groups.map((g) => (
                    <button
                      key={g.reason_code}
                      type="button"
                      onClick={() => navigate(`/register?reason_code=${encodeURIComponent(g.reason_code)}`)}
                      className="flex w-full items-center justify-between border-b border-reco-row py-[7px] text-left last:border-b-0"
                    >
                      <span className="text-[12px] text-reco-accent">{g.reason_code}</span>
                      <span className="font-mono text-[10.5px] text-reco-t5">
                        {g.count} · {formatRupees(g.total_exposure)}
                      </span>
                    </button>
                  ))}
                  <div className="mt-2 text-[10.5px] leading-snug text-reco-t5">
                    An invoice can appear under two reason codes, so these add up to more than
                    the period total.
                  </div>
                </div>
              )}
            </div>
          </div>

          <div className="mt-3.5 text-[11px] leading-relaxed text-reco-t5">
            {strings.dashboardScopeNote}
          </div>
        </>
      )}
    </div>
  );
}

function Card({
  label,
  value,
  sub,
  color,
  border,
}: {
  readonly label: string;
  readonly value: string;
  readonly sub: string;
  readonly color: string;
  readonly border: string;
}) {
  return (
    <div className="rounded-[10px] border bg-white p-4" style={{ borderColor: border }}>
      <div className="mb-2 font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">{label}</div>
      <div className="font-mono text-[26px] tracking-tight" style={{ color }}>
        {value}
      </div>
      <div className="mt-1.5 text-[11.5px] text-reco-t4">{sub}</div>
    </div>
  );
}
