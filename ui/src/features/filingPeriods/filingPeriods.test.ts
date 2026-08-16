/** Pure logic behind the Filing Periods console surface — Plan 107
 *  Slice 4 (`plans/107-filing-period.md`). The component itself is a
 *  thin fetch-and-render shell, the same posture `obligationCalendar.tsx`
 *  already takes; what is worth pinning here is which registered query
 *  a picker selection maps to, and how `period-list`'s raw N-Triples
 *  rows become options a human reads. */

import { describe, expect, it } from "vitest";
import type { SparqlResult } from "../../api";
import type { Solution } from "../../workbench/results";
import { hasDiffColumn, periodsFromRows, planPeriodQuery } from "./filingPeriods";

function getPeriodListRow(overrides: Partial<Solution> = {}): Solution {
  return {
    period: "<https://graph-owl.dev/packs/gst#period-2020-07>",
    periodLabel: '"2020-07"',
    ...overrides,
  };
}

describe("planPeriodQuery", () => {
  it("plans nothing when no period is picked", () => {
    expect(planPeriodQuery(null, null)).toBeNull();
  });

  it("plans period-summary when exactly one period is picked", () => {
    expect(planPeriodQuery("https://graph-owl.dev/packs/gst#period-2020-07", null)).toEqual({
      name: "period-summary",
      bindings: { period: "https://graph-owl.dev/packs/gst#period-2020-07" },
    });
  });

  it("plans period-diff when two distinct periods are picked", () => {
    expect(
      planPeriodQuery(
        "https://graph-owl.dev/packs/gst#period-2020-07",
        "https://graph-owl.dev/packs/gst#period-2026-07",
      ),
    ).toEqual({
      name: "period-diff",
      bindings: {
        periodA: "https://graph-owl.dev/packs/gst#period-2020-07",
        periodB: "https://graph-owl.dev/packs/gst#period-2026-07",
      },
    });
  });

  it("plans period-summary, not a degenerate diff, when the same period is picked twice", () => {
    // period-diff.sparql already guards duplicate VALUES rows with
    // SELECT DISTINCT (Slice 2's own mutation-tested fix), so a
    // self-diff would not be *wrong* — but it is a pointless request
    // when period-summary already answers the identical question in
    // one bound period instead of two.
    expect(
      planPeriodQuery(
        "https://graph-owl.dev/packs/gst#period-2020-07",
        "https://graph-owl.dev/packs/gst#period-2020-07",
      ),
    ).toEqual({
      name: "period-summary",
      bindings: { period: "https://graph-owl.dev/packs/gst#period-2020-07" },
    });
  });
});

describe("periodsFromRows", () => {
  it("returns an empty list for no rows", () => {
    expect(periodsFromRows([])).toEqual([]);
  });

  it("turns a period-list row into a clean iri/label pair", () => {
    const rows = [getPeriodListRow()];
    expect(periodsFromRows(rows)).toEqual([
      { iri: "https://graph-owl.dev/packs/gst#period-2020-07", label: "2020-07" },
    ]);
  });

  it("preserves period-list's own ORDER BY — does not re-sort", () => {
    const rows = [
      getPeriodListRow({
        period: "<https://graph-owl.dev/packs/gst#period-2026-08>",
        periodLabel: '"2026-08"',
      }),
      getPeriodListRow({
        period: "<https://graph-owl.dev/packs/gst#period-2020-07>",
        periodLabel: '"2020-07"',
      }),
    ];
    expect(periodsFromRows(rows).map((p) => p.label)).toEqual(["2026-08", "2020-07"]);
  });
});

function getResult(variables: readonly string[]): SparqlResult {
  return {
    rows: [],
    factsScanned: 0,
    truncated: false,
    asOf: null,
    plan: [],
    variables,
    federatedEndpoints: [],
    silencedFailures: [],
    alignmentsUsed: [],
  };
}

describe("hasDiffColumn", () => {
  it("is false with no result loaded yet", () => {
    expect(hasDiffColumn(null)).toBe(false);
  });

  it("is false for a period-summary result — subject/type only, no onlyIn", () => {
    expect(hasDiffColumn(getResult(["subject", "type"]))).toBe(false);
  });

  it("is true for a period-diff result — the actual loaded shape, not the picker's own mode", () => {
    // This is the bug a naive `plan?.name === "period-diff"` check hits:
    // that flag flips synchronously the instant a second period is
    // picked, one render before the async period-diff fetch resolves,
    // so the table would try to read `row.onlyIn` off rows that are
    // still period-summary's own shape. Deriving from the *loaded*
    // result's own `variables` instead ties the column set to data that
    // is actually there.
    expect(hasDiffColumn(getResult(["subject", "type", "onlyIn"]))).toBe(true);
  });
});
