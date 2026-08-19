import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { WhyPopover } from "../components/WhyPopover";
import { fetchItcPosition, type ItcPosition } from "../lib/api";
import { formatRupees } from "../lib/format";
import { loadStateFor } from "../lib/loadState";
import type { WorkspaceState } from "../lib/workspace";

/** Where the period's credit stands — the same five classes the Reconcile
 *  screen reports, from the same computation.
 *
 *  **This screen used to disagree with that one.** It summed `case_record`:
 *  only the flagged invoices, double-counting any invoice with two findings,
 *  excluding every clean one — then showed the result as though it were a
 *  period total. Every figure now carries how it was derived and what to do,
 *  because "blocked" and "pending" are the same size of number and opposite
 *  situations. */
const CLASSES = [
  { key: "confirmed", label: "Confirmed", colour: "#2f6b4d" },
  { key: "pending", label: "Pending", colour: "#a86a2c" },
  { key: "under_review", label: "Under review", colour: "#a86a2c" },
  { key: "blocked", label: "Blocked", colour: "#a13f28" },
  { key: "unclaimed", label: "Unclaimed", colour: "#41508f" },
] as const;

export default function ItcRoute() {
  const { clientId, periodId } = useOutletContext<WorkspaceState>();
  const [itc, setItc] = useState<ItcPosition | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!clientId || !periodId) return;
    setLoading(true);
    fetchItcPosition(clientId, periodId)
      .then(setItc)
      .catch(() => setItc(null))
      .finally(() => setLoading(false));
  }, [clientId, periodId]);

  const state = loadStateFor({ clientId, periodId, loading, data: itc });
  if (state === "no-workspace")
    return <div className="p-6 text-[13px] text-reco-t4">Choose a client and a period.</div>;
  if (state === "loading") return <div className="p-6 text-[13px] text-reco-t4">Loading…</div>;
  if (!itc) return <div className="p-6 text-[13px] text-reco-t4">No data for this period yet.</div>;

  return (
    <div className="space-y-6 p-6">
      <header>
        <h1 className="text-[19px] font-medium text-reco-t1">ITC position</h1>
        <p className="mt-1 text-[13px] text-reco-t4">
          Every rupee of credit, and where it stands. Hover any figure for how it was worked out.
        </p>
      </header>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
        {CLASSES.map(({ key, label, colour }) => (
          <div key={key} className="rounded-[10px] border border-reco-line bg-white p-4">
            <div className="mb-2 flex items-center font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
              {label.toUpperCase()}
              <WhyPopover title={label} explanation={itc.explain?.[key]} />
            </div>
            <div className="font-mono text-[22px]" style={{ color: colour }}>
              {formatRupees(itc.position[key] ?? 0)}
            </div>
          </div>
        ))}
      </div>

      {/* Why this screen's total differs from the working paper's. Two correct
          numbers measuring different populations look like a bug unless each
          says what it counted. */}
      {itc.compare_note && (
        <p className="rounded border border-reco-line bg-reco-panel-2 px-4 py-3 text-[12px] leading-relaxed text-reco-t3">
          {itc.compare_note}
        </p>
      )}
    </div>
  );
}
