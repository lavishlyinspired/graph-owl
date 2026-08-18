import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { fetchDashboard, type Dashboard } from "../lib/api";
import { strings } from "../lib/strings";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number): string {
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

export default function HomeRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchDashboard(clientId, periodId)
      .then(setDashboard)
      .catch(() => setDashboard(null));
  }, [clientId, periodId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="p-6">
      <h1 className="mb-1 text-[22px] font-bold tracking-tight text-reco-t1">{strings.dashboardTitle}</h1>
      <p className="mb-5 text-[12.5px] text-reco-t4">{strings.dashboardDesc}</p>

      <div className="mb-4 grid grid-cols-3 gap-3.5">
        <div className="rounded-lg border border-reco-line bg-reco-panel px-4.5 py-4">
          <div className="mb-2 font-mono text-[9.5px] tracking-[0.13em] text-reco-t4">{strings.dashboardCaseCount}</div>
          <div className="font-mono text-[27px] font-medium text-reco-t1">{dashboard?.case_count ?? "—"}</div>
        </div>
        <div className="rounded-lg border border-reco-bad-border bg-reco-panel px-4.5 py-4">
          <div className="mb-2 font-mono text-[9.5px] tracking-[0.13em] text-reco-t4">{strings.dashboardExposure}</div>
          <div className="font-mono text-[27px] font-medium text-reco-bad">
            {dashboard ? formatRupees(dashboard.total_exposure) : "—"}
          </div>
        </div>
        <div className="rounded-lg border border-reco-amber-border bg-reco-panel px-4.5 py-4">
          <div className="mb-2 font-mono text-[9.5px] tracking-[0.13em] text-reco-t4">{strings.dashboardApprovals}</div>
          <div className="font-mono text-[27px] font-medium text-reco-amber">{dashboard?.pending_approvals ?? "—"}</div>
        </div>
      </div>

      <div className="mb-4 overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
        <div className="border-b border-reco-line bg-reco-panel-2 px-4.5 py-2.5 text-[13px] font-semibold text-reco-t1">
          {strings.dashboardNeedsDecision}
        </div>
        {dashboard && dashboard.needs_decision.length === 0 && (
          <div className="px-4.5 py-4 text-[12.5px] text-reco-t4">{strings.dashboardEmpty}</div>
        )}
        {dashboard?.needs_decision.map((c) => (
          <div
            key={c.invoice_no}
            className="flex items-center gap-3.5 border-b border-reco-line-2 px-4.5 py-3 last:border-b-0"
          >
            <div className="flex-1">
              <div className="text-[13px] font-medium text-reco-t1">{c.invoice_no}</div>
              <div className="mt-0.5 text-[11.5px] text-reco-t4">
                {c.supplier_name ?? "—"} · {c.reason_code ?? "no reason yet"}
              </div>
            </div>
            <div className="font-mono text-[13px] text-reco-t1">{formatRupees(c.exposure)}</div>
          </div>
        ))}
      </div>

      <div className="rounded-lg border border-reco-accent-border bg-reco-accent-bg px-4.5 py-3 text-[11.5px] text-reco-t3">
        {strings.dashboardScopeNote}
      </div>
    </div>
  );
}
