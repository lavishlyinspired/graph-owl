import { ClientSwitcher } from "./ClientSwitcher";
import { PeriodPicker } from "./PeriodPicker";
import { strings } from "../lib/strings";

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
        <span className="text-[12px] text-reco-t4">{strings.askPlaceholder}</span>
        <span className="ml-auto rounded border border-reco-line px-1.5 py-0.5 font-mono text-[9.5px] text-reco-t5">
          {strings.askShortcut}
        </span>
      </button>

      <div className="ml-auto flex items-center gap-3">
        <div className="flex items-center gap-[7px] rounded-[6px] border border-[#dfe3f2] bg-[#f4f6fb] px-2.5 py-[5px]">
          <span className="h-[7px] w-[7px] rounded-[2px] bg-reco-accent" />
          <span className="font-mono text-[10px] tracking-[.04em] text-[#41508f]">
            GRAPHOWL · GST PACK 1.4.2
          </span>
        </div>
        <div className="flex items-center gap-[7px] rounded-[6px] border border-[#f0dcc2] bg-[#fdf3e7] px-[11px] py-[5px]">
          <span className="font-mono text-[10px] tracking-[.05em] text-[#a86a2c]">
            ITC AT RISK
          </span>
          <span className="font-mono text-[12.5px] font-medium text-[#a13f28]">
            ₹12.4L
          </span>
        </div>
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
