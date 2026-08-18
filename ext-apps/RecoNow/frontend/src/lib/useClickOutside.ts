import { useEffect, type RefObject } from "react";

/** Found live, not in a test: `ClientSwitcher`'s and `PeriodPicker`'s
 *  dropdowns only closed via their own trigger button, so a click meant for
 *  the Ask panel underneath (or a stray click anywhere else) landed inside
 *  the still-open dropdown instead — typed text went into a "Month" field
 *  behind the Ask panel rather than the Ask input. */
export function useClickOutside(ref: RefObject<HTMLElement | null>, onOutside: () => void, active: boolean): void {
  useEffect(() => {
    if (!active) return;
    const handler = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) onOutside();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [ref, onOutside, active]);
}
