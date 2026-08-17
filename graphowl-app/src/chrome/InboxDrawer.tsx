import { strings } from "../lib/strings";

export interface InboxItem {
  readonly id: string;
  readonly tag: string;
  readonly title: string;
  readonly detail: string;
  readonly who: string;
}

interface InboxDrawerProps {
  readonly open: boolean;
  readonly items: readonly InboxItem[];
  readonly onClose: () => void;
  readonly onApprove: (id: string) => void;
  readonly onReject: (id: string) => void;
}

export function InboxDrawer({ open, items, onClose, onApprove, onReject }: InboxDrawerProps) {
  if (!open) return null;

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

      <div className="flex-1 overflow-y-auto">
        {items.length === 0 && <div className="p-4 text-[13px] text-gowl-t5">{strings.inboxEmpty}</div>}
        {items.map((item) => (
          <div key={item.id} className="border-b border-gowl-line p-4">
            <div className="font-mono text-[10px] tracking-widest text-gowl-t6">{item.tag}</div>
            <div className="mt-1 text-[13px] font-medium text-gowl-t1">{item.title}</div>
            <div className="mt-1 text-[12px] text-gowl-t5">{item.detail}</div>
            <div className="mt-2 flex items-center justify-between">
              <span className="text-[11px] text-gowl-t6">{item.who}</span>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => onReject(item.id)}
                  className="rounded border border-gowl-line px-2 py-1 text-[12px] text-gowl-t3 hover:border-gowl-bad hover:text-gowl-bad"
                >
                  {strings.reject}
                </button>
                <button
                  type="button"
                  onClick={() => onApprove(item.id)}
                  className="rounded border border-gowl-accent-border bg-gowl-accent-bg px-2 py-1 text-[12px] text-gowl-accent hover:border-gowl-accent"
                >
                  {strings.approve}
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="border-t border-gowl-line p-3 text-[11px] text-gowl-t6">{strings.inboxFooter}</div>
    </div>
  );
}
