import { useEffect, useState } from "react";
import { decideApproval, fetchApprovals, type Approval } from "../lib/api";
import { strings } from "../lib/strings";

interface InboxDrawerProps {
  readonly clientId: string;
  readonly periodId: string;
  readonly onClose: () => void;
  readonly onDecided: () => void;
}

export function InboxDrawer({ clientId, periodId, onClose, onDecided }: InboxDrawerProps) {
  const [items, setItems] = useState<readonly Approval[]>([]);

  const refresh = () => {
    fetchApprovals(clientId, periodId)
      .then(setItems)
      .catch(() => setItems([]));
  };

  useEffect(refresh, [clientId, periodId]);

  const decide = async (id: string, status: "approved" | "rejected") => {
    await decideApproval(clientId, periodId, id, status);
    refresh();
    onDecided();
  };

  return (
    <div className="fixed top-[56px] right-4 z-[70] w-[430px] overflow-hidden rounded-[10px] border border-reco-line-3 bg-reco-panel shadow-2xl">
      <div className="flex items-center border-b border-reco-line-2 px-4 py-3">
        <span className="text-[13px] font-semibold text-reco-t1">{strings.inboxTitle}</span>
        <span className="ml-2.5 text-[11.5px] text-reco-t4">{strings.inboxSubtitle}</span>
        <button type="button" onClick={onClose} className="ml-auto cursor-pointer text-reco-t5">
          ×
        </button>
      </div>

      {items.length === 0 && <div className="px-4 py-4 text-[12.5px] text-reco-t4">{strings.inboxEmpty}</div>}

      {items.map((item) => (
        <div key={item.id} className="border-b border-reco-line-2 px-4 py-3">
          <div className="mb-1 text-[12.5px] font-medium text-reco-t1">{item.decision_type}</div>
          {item.amount != null && <div className="mb-1 font-mono text-[11.5px] text-reco-t3">₹{item.amount}</div>}
          <div className="mt-2 flex justify-end gap-1.5">
            <button
              type="button"
              onClick={() => decide(item.id, "rejected")}
              className="rounded-md border border-reco-bad-border px-2.5 py-1 text-[11px] text-reco-bad"
            >
              {strings.inboxReject}
            </button>
            <button
              type="button"
              onClick={() => decide(item.id, "approved")}
              className="rounded-md bg-reco-t0 px-2.5 py-1 text-[11px] text-white"
            >
              {strings.inboxApprove}
            </button>
          </div>
        </div>
      ))}

      <div className="px-4 py-2.5 text-[11.5px] text-reco-t4">{strings.inboxFooter}</div>
    </div>
  );
}
