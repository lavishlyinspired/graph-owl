import { useEffect, useState } from "react";
import { Link, useOutletContext } from "react-router-dom";
import { fetchExceptions, type ExceptionGroup } from "../lib/api";
import type { WorkspaceState } from "../lib/workspace";

function formatRupees(amount: number): string {
  return `₹${amount.toLocaleString("en-IN", { maximumFractionDigits: 0 })}`;
}

export default function ExceptionsRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [groups, setGroups] = useState<readonly ExceptionGroup[]>([]);

  useEffect(() => {
    if (!clientId || !periodId) return;
    fetchExceptions(clientId, periodId)
      .then(setGroups)
      .catch(() => setGroups([]));
  }, [clientId, periodId]);

  if (!clientId || !periodId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client and a period first.</div>;
  }

  return (
    <div className="p-6">
      <h1 className="mb-1 text-[20px] font-bold text-reco-t1">Exceptions by reason</h1>
      <p className="mb-4 text-[12.5px] text-reco-t4">
        Reason groups for the whole period. Pick a group to get its invoices.
      </p>

      <div className="overflow-hidden rounded-lg border border-reco-line bg-reco-panel">
        <div className="grid grid-cols-[1.5fr_100px_140px] gap-3 border-b border-reco-line bg-reco-panel-2 px-4 py-2 font-mono text-[9.5px] tracking-[0.1em] text-reco-t5">
          <span>REASON</span>
          <span className="text-right">COUNT</span>
          <span className="text-right">EXPOSURE</span>
        </div>
        {groups.length === 0 && <div className="px-4 py-4 text-[12.5px] text-reco-t4">No exceptions yet.</div>}
        {groups.map((g) => (
          <Link
            key={g.reason_code}
            to={`/register?reason_code=${encodeURIComponent(g.reason_code)}`}
            className="grid grid-cols-[1.5fr_100px_140px] items-center gap-3 border-b border-reco-line-2 px-4 py-3 text-[12.5px] hover:bg-reco-panel-2"
          >
            <span className="text-reco-t1">{g.reason_code}</span>
            <span className="text-right font-mono text-reco-t2">{g.count}</span>
            <span className="text-right font-mono text-reco-bad">{formatRupees(g.total_exposure)}</span>
          </Link>
        ))}
      </div>
    </div>
  );
}
