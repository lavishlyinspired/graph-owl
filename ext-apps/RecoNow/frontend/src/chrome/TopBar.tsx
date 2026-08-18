import { useEffect, useState } from "react";
import { ClientSwitcher } from "./ClientSwitcher";
import { PeriodPicker } from "./PeriodPicker";
import { strings } from "../lib/strings";
import { fetchDashboard, fetchGraphOwlStatus, type GraphOwlStatus } from "../lib/api";

/** ₹12,34,567 → "₹12.35L". Indian lakh/crore grouping, because that is how
 *  the figure is read by the people who read it. */
function formatCompactRupees(amount: number): string {
  if (amount >= 10_000_000) return `₹${(amount / 10_000_000).toFixed(2)}Cr`;
  if (amount >= 100_000) return `₹${(amount / 100_000).toFixed(2)}L`;
  return `₹${Math.round(amount).toLocaleString("en-IN")}`;
}

interface TopBarProps {
  readonly clientId: string | null;
  readonly periodId: string | null;
  readonly onSelectClient: (id: string) => void;
  readonly onSelectPeriod: (id: string) => void;
  readonly onOpenAsk: () => void;
  readonly onOpenInbox: () => void;
  readonly pendingCount: number;
}

export function TopBar({
  clientId,
  periodId,
  onSelectClient,
  onSelectPeriod,
  onOpenAsk,
  onOpenInbox,
  pendingCount,
}: TopBarProps) {
  // Both of these were literals in the mockup — a pack version that was
  // reported whether or not a pack was installed, and an ITC-at-risk figure
  // that never changed. A header that states a number is claiming to know it.
  const [status, setStatus] = useState<GraphOwlStatus | null>(null);
  const [atRisk, setAtRisk] = useState<number | null>(null);

  useEffect(() => {
    fetchGraphOwlStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    if (!clientId || !periodId) {
      setAtRisk(null);
      return;
    }
    let cancelled = false;
    fetchDashboard(clientId, periodId)
      .then((d) => {
        if (!cancelled) setAtRisk(d.total_exposure);
      })
      .catch(() => {
        if (!cancelled) setAtRisk(null);
      });
    return () => {
      cancelled = true;
    };
  }, [clientId, periodId]);

  return (
    <header className="flex h-14 flex-none items-center gap-4.5 border-b border-reco-line bg-reco-panel px-5">
      <div className="flex items-center gap-2.5">
        <div className="flex h-5.5 w-5.5 items-center justify-center rounded-md bg-reco-t0 text-[11px] font-bold text-white">
          R
        </div>
        <span className="text-[14.5px] font-bold tracking-tight text-reco-t1">{strings.brand}</span>
      </div>
      <div className="h-5.5 w-px bg-reco-line" />

      <ClientSwitcher clientId={clientId} onSelect={onSelectClient} />
      <PeriodPicker clientId={clientId} periodId={periodId} onSelect={onSelectPeriod} />

      <button
        type="button"
        onClick={onOpenAsk}
        disabled={!clientId || !periodId}
        className="flex h-8 max-w-[330px] flex-1 items-center gap-2.5 rounded-md border border-reco-line bg-reco-panel-2 px-2.5 disabled:opacity-50"
      >
        <span className="text-[12px] text-reco-t5">⌕</span>
        <span className="text-[12px] text-reco-t4">{strings.askButtonLabel}</span>
        <span className="ml-auto rounded border border-reco-line px-1.5 py-0.5 font-mono text-[9.5px] text-reco-t5">
          {strings.askShortcut}
        </span>
      </button>

      <div className="ml-auto flex items-center gap-3">
        <div
          className={`flex items-center gap-[7px] rounded-[6px] border px-2.5 py-[5px] ${
            status?.reachable
              ? "border-[#dfe3f2] bg-[#f4f6fb]"
              : "border-reco-bad-border bg-reco-bad-bg"
          }`}
          title={status ? `graph-owl at ${status.server}` : undefined}
        >
          <span
            className={`h-[7px] w-[7px] rounded-[2px] ${
              status?.reachable ? "bg-reco-accent" : "bg-reco-bad"
            }`}
          />
          <span
            className={`font-mono text-[10px] tracking-[.04em] ${
              status?.reachable ? "text-[#41508f]" : "text-reco-bad"
            }`}
          >
            {status === null
              ? "GRAPHOWL · CHECKING"
              : !status.reachable
                ? "GRAPHOWL · UNREACHABLE"
                : status.pack
                  ? `GRAPHOWL · ${status.pack.id.toUpperCase()} PACK ${status.pack.version}`
                  : "GRAPHOWL · NO PACK"}
          </span>
        </div>
        {/* Omitted rather than zeroed when there is no period: "₹0 at risk"
            is a finding, and nothing has been reconciled to support it. */}
        {atRisk !== null && (
          <div className="flex items-center gap-[7px] rounded-[6px] border border-[#f0dcc2] bg-[#fdf3e7] px-[11px] py-[5px]">
            <span className="font-mono text-[10px] tracking-[.05em] text-[#a86a2c]">
              {strings.itcAtRiskLabel}
            </span>
            <span className="font-mono text-[12.5px] font-medium text-[#a13f28]">
              {formatCompactRupees(atRisk)}
            </span>
          </div>
        )}
        <button
          type="button"
          onClick={onOpenInbox}
          className="relative flex items-center gap-[7px] rounded-[6px] border border-reco-line px-2.5 py-[5px] hover:border-reco-line-3"
        >
          <span className="text-[12px] text-reco-t4">⚑</span>
          <span className="text-[11.5px] text-reco-t2">{strings.inboxTitle}</span>
          {pendingCount > 0 && (
            <span className="flex h-4 min-w-[16px] items-center justify-center rounded-full bg-reco-bad px-1 font-mono text-[9.5px] text-white">
              {pendingCount}
            </span>
          )}
        </button>
        <div className="flex h-7 w-7 items-center justify-center rounded-full bg-reco-line-2 text-[10.5px] font-semibold text-reco-t2">
          —
        </div>
      </div>
    </header>
  );
}
