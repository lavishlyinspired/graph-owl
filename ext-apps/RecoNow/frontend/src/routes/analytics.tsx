import { useEffect, useState } from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import { fetchAnalytics, type Analytics } from "../lib/api";
import { formatRupees } from "../lib/format";
import type { WorkspaceState } from "../lib/workspace";

/** Exposure per period, for the periods this client actually has.
 *
 *  This screen previously drew a five-month Apr–Aug series (₹8.2L, ₹5.4L,
 *  ₹6.7L, ₹4.9L, ₹4.2L) and an insight reading "Match rate improved 6 points
 *  since April, and follow-up volume fell 75%" — for a client whose only
 *  reconciled period is March 2026. Neither the series nor the claim came
 *  from a query.
 *
 *  A trend needs more than one period. Where there is one, the honest
 *  analytic is that one figure plus a note saying what would make it a
 *  trend. */
export default function AnalyticsRoute() {
  const { clientId } = useOutletContext<WorkspaceState>();
  const [data, setData] = useState<Analytics | null>(null);
  const [failed, setFailed] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    if (!clientId) return;
    let cancelled = false;
    setFailed(false);
    fetchAnalytics(clientId)
      .then((d) => !cancelled && setData(d))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [clientId]);

  if (!clientId) {
    return <div className="p-8 text-[13px] text-reco-t4">Select a client first.</div>;
  }
  if (failed) {
    return <div className="p-8 text-[13px] text-reco-bad">Could not load analytics.</div>;
  }
  if (!data) {
    return <div className="p-8 text-[13px] text-reco-t4">Loading…</div>;
  }

  const peak = Math.max(1, ...data.periods.map((p) => p.exposure));

  return (
    <div className="p-6 pb-11">
      <div className="mb-4">
        <h1 className="mb-1 text-[20px] font-bold tracking-tight text-reco-t1">Analytics</h1>
        <p className="text-[12.5px] text-reco-t4">ITC at risk per period, for this client.</p>
      </div>

      {data.periods.length === 0 ? (
        <div className="rounded-[10px] border border-reco-line bg-white px-5 py-10 text-center">
          <div className="text-[13px] text-reco-t2">No periods yet for this client.</div>
        </div>
      ) : (
        <>
          <div className="mb-3.5 rounded-[10px] border border-reco-line bg-white p-4 pb-[18px]">
            <div className="mb-4 flex items-baseline justify-between">
              <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-t4">
                ITC AT RISK PER PERIOD
              </span>
              <span className="text-[11.5px] text-reco-t5">
                bar height is exposure, relative to the largest period shown
              </span>
            </div>
            <div className="flex h-[150px] items-end gap-3">
              {data.periods.map((p) => (
                <button
                  key={p.period_id}
                  type="button"
                  onClick={() => navigate("/register")}
                  className="flex h-full flex-1 flex-col items-center justify-end gap-[7px]"
                  title={`${p.case_count} case(s)`}
                >
                  <span className="font-mono text-[10.5px] text-reco-t2">
                    {formatRupees(p.exposure)}
                  </span>
                  <div
                    className="w-full rounded-t-[4px] bg-reco-t0"
                    style={{ height: `${Math.max(4, (p.exposure / peak) * 100)}%` }}
                  />
                  <span className="font-mono text-[10px] text-reco-t5">{p.label}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="grid grid-cols-[1fr_320px] items-start gap-3.5">
            <div className="overflow-hidden rounded-[10px] border border-reco-line bg-white">
              <div className="grid grid-cols-[1.4fr_100px_140px_110px] gap-3 border-b border-reco-line bg-reco-panel-2 px-[18px] py-2.5 font-mono text-[9.5px] tracking-[0.1em] text-reco-t4">
                <span>PERIOD</span>
                <span>CASES</span>
                <span>ITC AT RISK</span>
                <span>STATUS</span>
              </div>
              {data.periods.map((p) => (
                <div
                  key={p.period_id}
                  className="grid grid-cols-[1.4fr_100px_140px_110px] gap-3 border-b border-reco-row px-[18px] py-3 last:border-b-0"
                >
                  <span className="text-[12.5px] text-reco-t1">{p.label}</span>
                  <span className="font-mono text-[12px] text-reco-t2">{p.case_count}</span>
                  <span className="font-mono text-[12px] text-reco-t1">
                    {formatRupees(p.exposure)}
                  </span>
                  <span className="font-mono text-[11px] text-reco-t4">{p.status}</span>
                </div>
              ))}
            </div>

            <div className="rounded-[10px] border border-reco-accent-border bg-white p-4">
              <div className="mb-2.5 flex items-center gap-2">
                <span className="h-[7px] w-[7px] rounded-[2px] bg-reco-accent" />
                <span className="font-mono text-[9.5px] tracking-[0.12em] text-reco-accent-hi">
                  WHAT THIS CAN SHOW
                </span>
              </div>
              <div className="text-[12.5px] leading-relaxed text-reco-t2">
                {data.has_trend
                  ? "Each bar is one reconciled period's ITC at risk, counting every invoice once. Movement between periods is the movement in exposure, not in match rate — match rate needs per-invoice history this backend does not keep yet."
                  : "One reconciled period is a figure, not a trend. Reconcile a second period and this becomes a comparison; until then there is nothing to compare it against."}
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
