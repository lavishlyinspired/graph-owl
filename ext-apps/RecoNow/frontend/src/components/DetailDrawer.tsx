import { useEffect, type ReactNode } from "react";

/** The right-side detail panel, shared.
 *
 *  **Why a drawer and not an inline expansion.** Clicking a finding used to
 *  expand a row in place, or filter a table further down the page — either way
 *  the answer arrived somewhere the reader had to go and find. A drawer puts
 *  it at a fixed place on screen, so the click and its result are in the same
 *  visual moment, and the list underneath stays put as context.
 *
 *  Geometry matches the drawer `GenericScreen` already uses for IMS and
 *  Follow-ups — one detail affordance across the product, not three that each
 *  look nearly the same. */
export function DetailDrawer({
  open,
  title,
  subtitle,
  onClose,
  children,
  footer,
}: {
  readonly open: boolean;
  readonly title: string;
  readonly subtitle?: string;
  readonly onClose: () => void;
  readonly children: ReactNode;
  readonly footer?: ReactNode;
}) {
  // Escape closes it. A panel that can only be dismissed by finding a small ×
  // is one that traps a keyboard user.
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <aside
      role="complementary"
      aria-label={title}
      className="fixed inset-y-0 right-0 top-14 z-30 flex w-[392px] flex-col border-l border-reco-line bg-white shadow-[-18px_0_42px_rgba(28,27,24,.12)]"
    >
      <div className="flex items-start gap-3 border-b border-reco-line-2 px-5 py-[18px]">
        <div className="flex-1">
          <div className="mb-1 text-[15px] font-bold leading-snug text-reco-t1">{title}</div>
          {subtitle && <div className="font-mono text-[10.5px] text-reco-t4">{subtitle}</div>}
        </div>
        <button
          type="button"
          aria-label="Close"
          className="text-[15px] leading-none text-reco-t5 hover:text-reco-t2"
          onClick={onClose}
        >
          ×
        </button>
      </div>

      <div className="flex-1 overflow-auto px-5 py-4">{children}</div>

      {footer && <div className="border-t border-reco-line-2 px-5 py-3">{footer}</div>}
    </aside>
  );
}
