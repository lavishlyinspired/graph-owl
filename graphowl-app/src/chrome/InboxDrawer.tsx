import { useState } from "react";
import { strings } from "../lib/strings";
import { INBOX_REJECT_REQUIRES_REASON } from "../lib/inboxActions";
import type { InboxItem } from "../lib/api";

interface InboxDrawerProps {
  readonly open: boolean;
  readonly items: readonly InboxItem[];
  readonly onClose: () => void;
  readonly onDecide: (item: InboxItem, decision: "approve" | "reject", reason?: string) => Promise<void>;
}

export function InboxDrawer({ open, items, onClose, onDecide }: InboxDrawerProps) {
  const [rejecting, setRejecting] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const startReject = (id: string) => {
    setRejecting(id);
    setReason("");
    setError(null);
  };

  const confirmReject = async (item: InboxItem) => {
    try {
      await onDecide(item, "reject", reason);
      setRejecting(null);
    } catch {
      setError(strings.inboxActionFailed);
    }
  };

  const approve = async (item: InboxItem) => {
    try {
      await onDecide(item, "approve");
    } catch {
      setError(strings.inboxActionFailed);
    }
  };

  return (
    <div
      role="dialog"
      aria-label={strings.inboxTitle}
      className="fixed inset-y-0 right-0 z-50 flex w-[380px] flex-col border-l border-gowl-line bg-gowl-panel shadow-2xl"
    >
      <div className="flex items-start justify-between border-b border-gowl-line p-4">
        <div>
          <div className="text-[14px] font-semibold text-gowl-t1">{strings.inboxTitle}</div>
          <div className="text-[12px] text-gowl-t5">{strings.inboxSubtitle}</div>
        </div>
        <button type="button" onClick={onClose} aria-label={strings.close} className="text-gowl-t5 hover:text-gowl-t1">
          {strings.closeIcon}
        </button>
      </div>

      {error && <div className="border-b border-gowl-bad-bg bg-gowl-bad-bg p-2 text-[12px] text-gowl-bad">{error}</div>}

      <div className="flex-1 overflow-y-auto">
        {items.length === 0 && <div className="p-4 text-[13px] text-gowl-t5">{strings.inboxEmpty}</div>}
        {items.map((item) => {
          const requiresReason = INBOX_REJECT_REQUIRES_REASON.has(item.source);
          const isRejecting = rejecting === item.id;
          return (
            <div key={item.id} className="border-b border-gowl-line p-4">
              <div className="font-mono text-[10px] tracking-widest text-gowl-t6">{item.tag}</div>
              <div className="mt-1 text-[13px] font-medium text-gowl-t1">{item.title}</div>
              <div className="mt-1 text-[12px] text-gowl-t5">{item.detail}</div>

              {isRejecting ? (
                <div className="mt-2">
                  <input
                    autoFocus
                    value={reason}
                    onChange={(e) => setReason(e.target.value)}
                    placeholder={strings.inboxReasonPlaceholder}
                    className="w-full rounded border border-gowl-line bg-gowl-input px-2 py-1 text-[12px] text-gowl-t1"
                  />
                  <div className="mt-2 flex justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => setRejecting(null)}
                      className="rounded border border-gowl-line px-2 py-1 text-[12px] text-gowl-t3"
                    >
                      {strings.inboxReasonCancel}
                    </button>
                    <button
                      type="button"
                      disabled={requiresReason && reason.trim().length === 0}
                      onClick={() => void confirmReject(item)}
                      className="rounded border border-gowl-bad bg-gowl-bad-bg px-2 py-1 text-[12px] text-gowl-bad disabled:opacity-40"
                    >
                      {strings.inboxReasonConfirm}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="mt-2 flex items-center justify-between">
                  <span className="text-[11px] text-gowl-t6">{item.who}</span>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      onClick={() => startReject(item.id)}
                      className="rounded border border-gowl-line px-2 py-1 text-[12px] text-gowl-t3 hover:border-gowl-bad hover:text-gowl-bad"
                    >
                      {strings.reject}
                    </button>
                    <button
                      type="button"
                      onClick={() => void approve(item)}
                      className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-2 py-1 text-[12px] text-gowl-accent hover:border-gowl-accent"
                    >
                      {strings.approve}
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className="border-t border-gowl-line p-3 text-[11px] text-gowl-t6">{strings.inboxFooter}</div>
    </div>
  );
}
