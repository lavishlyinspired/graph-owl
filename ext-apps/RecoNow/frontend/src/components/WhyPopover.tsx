import { useState } from "react";

/** The (i) affordance behind every figure and every rule label.
 *
 *  **The user's ask**: "how is this value derived, what's the calculation...
 *  you should have an explanation as a hover or an (i) popup. This has to be
 *  for all the screens."
 *
 *  Opens on hover *and* on click, and stays open on click. Hover alone is
 *  unusable on touch and unreachable by keyboard; click alone hides the
 *  affordance from anyone who does not know to try. The button is a real
 *  `<button>` so it is focusable and announced.
 *
 *  Content comes from the API, never from a constant here — the same figure
 *  appears on several screens, and two screens explaining one number
 *  differently is worse than neither explaining it. */
export interface Explanation {
  readonly means?: string | null;
  readonly formula?: string | null;
  readonly action?: string | null;
  readonly source?: string | null;
  /** Used when the subject is a rule rather than a figure. */
  readonly meaning?: string | null;
  readonly next_action?: string | null;
}

export function WhyPopover({
  title,
  explanation,
  align = "left",
}: {
  readonly title: string;
  readonly explanation: Explanation | null | undefined;
  readonly align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);

  if (!explanation) return null;

  const means = explanation.means ?? explanation.meaning;
  const action = explanation.action ?? explanation.next_action;
  const visible = open || pinned;

  return (
    <span className="relative inline-block">
      <button
        type="button"
        aria-label={`Why: ${title}`}
        aria-expanded={visible}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setPinned((p) => !p)}
        className="ml-1 inline-flex h-[14px] w-[14px] items-center justify-center rounded-full border border-reco-line text-[9px] leading-none text-reco-t5 hover:border-reco-accent hover:text-reco-accent"
      >
        i
      </button>

      {visible && (
        <span
          role="tooltip"
          className={`absolute z-30 mt-1 block w-[320px] rounded border border-reco-line bg-reco-panel p-3 text-left shadow-lg ${
            align === "right" ? "right-0" : "left-0"
          }`}
        >
          <span className="block text-[12px] font-medium text-reco-t1">{title}</span>
          {means && <span className="mt-1 block text-[11.5px] leading-relaxed text-reco-t2">{means}</span>}
          {explanation.formula && (
            <>
              <span className="mt-2 block text-[9.5px] uppercase tracking-wider text-reco-t5">
                How it is worked out
              </span>
              <span className="block text-[11px] leading-relaxed text-reco-t3">
                {explanation.formula}
              </span>
            </>
          )}
          {action && (
            <>
              <span className="mt-2 block text-[9.5px] uppercase tracking-wider text-reco-t5">
                What to do
              </span>
              <span className="block text-[11px] leading-relaxed text-reco-t3">{action}</span>
            </>
          )}
          {explanation.source && (
            <span className="mt-2 block text-[10.5px] text-reco-t5">
              Source: {explanation.source}
            </span>
          )}
        </span>
      )}
    </span>
  );
}
