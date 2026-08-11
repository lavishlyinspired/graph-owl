/** Pure logic behind the obligation calendar — Epic 105 P8/F4
 *  (`plans/105h-obligation-calendar.md`). The component itself is a thin
 *  fetch-and-render shell; what is worth pinning here is how a raw
 *  `Obligation[]` becomes what a reviewer reads: status buckets and the
 *  `?window=` filter. */

import { describe, expect, it } from "vitest";
import type { Obligation } from "../../api";
import { obligationStatus, withinWindow } from "./obligationCalendar";

function getObligation(overrides: Partial<Obligation> = {}): Obligation {
  return {
    pack: "gst",
    label: "gst:PaymentOverdue",
    subject: "https://graph-owl.dev/packs/gst#purchase-INV-1003",
    governedBy: "gst:Section16-2-d",
    anchor: "2026-01-01",
    due: "2026-06-30",
    daysRemaining: 10,
    ...overrides,
  };
}

describe("obligationStatus", () => {
  it("is overdue once days remaining goes negative", () => {
    expect(obligationStatus(-1)).toBe("overdue");
    expect(obligationStatus(-180)).toBe("overdue");
  });

  it("is dueSoon within the 30-day review horizon, inclusive of today", () => {
    expect(obligationStatus(0)).toBe("dueSoon");
    expect(obligationStatus(30)).toBe("dueSoon");
  });

  it("is upcoming beyond the 30-day horizon", () => {
    expect(obligationStatus(31)).toBe("upcoming");
  });
});

describe("withinWindow", () => {
  it("returns every obligation when no window is set", () => {
    const obligations = [getObligation({ daysRemaining: 5 }), getObligation({ daysRemaining: 400 })];
    expect(withinWindow(obligations, null)).toEqual(obligations);
  });

  it("keeps obligations due within the window", () => {
    const soon = getObligation({ subject: "soon", daysRemaining: 5 });
    const far = getObligation({ subject: "far", daysRemaining: 90 });
    expect(withinWindow([soon, far], 30)).toEqual([soon]);
  });

  it("keeps an already-overdue obligation regardless of the window — it needs attention now, not later", () => {
    const overdue = getObligation({ subject: "overdue", daysRemaining: -5 });
    expect(withinWindow([overdue], 30)).toEqual([overdue]);
  });

  it("includes an obligation due on exactly the window boundary", () => {
    const boundary = getObligation({ daysRemaining: 30 });
    expect(withinWindow([boundary], 30)).toEqual([boundary]);
  });

  it("excludes an obligation one day past the window boundary", () => {
    const justOutside = getObligation({ daysRemaining: 31 });
    expect(withinWindow([justOutside], 30)).toEqual([]);
  });
});
