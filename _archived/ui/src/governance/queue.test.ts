import { describe, expect, it } from "vitest";
import {
  type Finding,
  type Severity,
  type Waiver,
  SEVERITY_ORDER,
  isWaived,
  currency,
  describeSuggestion,
  groupByAsset,
  localName,
} from "./queue";

function finding(over: Partial<Finding> = {}): Finding {
  return {
    id: crypto.randomUUID(),
    shape: "1:TableShape",
    focusNode: "1:payments",
    path: "1:owner",
    constraint: "minCount",
    severity: "violation",
    message: "needs an owner",
    actual: null,
    suggestion: null,
    ...over,
  };
}

describe("severity order", () => {
  // Pinned, because every ordering assertion in this file rests on it and a
  // typo here would silently reclassify a severity as unrecognised — which
  // sorts it last, exactly where a violation must never be.
  it("runs loudest to quietest", () => {
    expect(SEVERITY_ORDER).toEqual(["violation", "warning", "info"]);
  });
});

describe("the queue groups by asset", () => {
  it("turns many findings about one asset into one row of work", () => {
    const rows = groupByAsset([
      finding({ constraint: "minCount" }),
      finding({ constraint: "maxCount" }),
      finding({ constraint: "pattern" }),
    ]);

    expect(rows).toHaveLength(1);
    expect(rows[0]!.focusNode).toBe("1:payments");
    expect(rows[0]!.findings).toHaveLength(3);
  });

  it("keeps different assets apart", () => {
    const rows = groupByAsset([
      finding({ focusNode: "1:a" }),
      finding({ focusNode: "1:b" }),
    ]);

    expect(rows.map((r) => r.focusNode)).toEqual(["1:a", "1:b"]);
  });

  // A queue is worked from the top. An asset with a violation must outrank one
  // with only advice, whatever order the server happened to return them in.
  it("orders the loudest asset first regardless of arrival order", () => {
    const rows = groupByAsset([
      finding({ focusNode: "1:quiet", severity: "info" }),
      finding({ focusNode: "1:advice", severity: "warning" }),
      finding({ focusNode: "1:broken", severity: "violation" }),
    ]);

    expect(rows.map((r) => r.focusNode)).toEqual(["1:broken", "1:advice", "1:quiet"]);
  });

  // A group's badge is its worst finding, not its first or its most common. An
  // asset with one violation and nine warnings needs attention.
  it("badges a group with its loudest finding", () => {
    const rows = groupByAsset([
      finding({ severity: "warning" }),
      finding({ severity: "warning" }),
      finding({ severity: "violation" }),
    ]);

    expect(rows[0]!.severity).toBe("violation");
  });

  // The loudest finding is not always the last one seen. A fold that simply
  // took the most recent value passed the case above by luck.
  it("badges a group whose loudest finding arrived first", () => {
    const rows = groupByAsset([
      finding({ severity: "violation" }),
      finding({ severity: "warning", constraint: "other" }),
    ]);

    expect(rows[0]!.severity).toBe("violation");
  });

  // And the negative: a group with nothing loud is not promoted.
  it("does not badge a group louder than its findings", () => {
    const rows = groupByAsset([finding({ severity: "info" }), finding({ severity: "info" })]);

    expect(rows[0]!.severity).toBe("info");
  });

  it("orders findings inside a group worst first too", () => {
    const rows = groupByAsset([
      finding({ severity: "info", constraint: "a" }),
      finding({ severity: "violation", constraint: "z" }),
    ]);

    expect(rows[0]!.findings.map((f) => f.severity)).toEqual(["violation", "info"]);
  });

  // Two polls of an unchanged queue must render identically, or nobody can
  // work it from the top — the row they were reading moves under them.
  it("is stable across identical inputs in a different order", () => {
    const findings = [
      finding({ focusNode: "1:c" }),
      finding({ focusNode: "1:a" }),
      finding({ focusNode: "1:b" }),
    ];

    const forwards = groupByAsset(findings).map((r) => r.focusNode);
    const backwards = groupByAsset([...findings].reverse()).map((r) => r.focusNode);

    expect(forwards).toEqual(["1:a", "1:b", "1:c"]);
    expect(backwards).toEqual(forwards);
  });

  it("has nothing to say about an empty queue", () => {
    expect(groupByAsset([])).toEqual([]);
  });

  // A severity this build does not know about must not push real violations
  // down the page. Sorting it first is exactly what a naive comparison does.
  it("sorts an unrecognised severity last rather than first", () => {
    const rows = groupByAsset([
      finding({ focusNode: "1:future", severity: "catastrophe" as Severity }),
      finding({ focusNode: "1:broken", severity: "violation" }),
    ]);

    expect(rows[0]!.focusNode).toBe("1:broken");
  });
});

describe("a suggestion says what to do", () => {
  it("names the field to add, with the shape's own reason", () => {
    expect(
      describeSuggestion({
        action: "assertMissing",
        path: "1:owner",
        hint: "at least 1 required, 0 present",
      }),
    ).toBe("Add owner — at least 1 required, 0 present");
  });

  it("still says something useful without a hint", () => {
    expect(describeSuggestion({ action: "assertMissing", path: "1:owner" })).toBe("Add owner");
  });

  it("names how many to keep when there are too many", () => {
    expect(describeSuggestion({ action: "retractExcess", path: "1:fqn", keep: 1 })).toBe(
      "Remove all but 1 fqn",
    );
  });

  it("names the type to store when the type is wrong", () => {
    expect(
      describeSuggestion({ action: "retypeValue", path: "1:ordinalPosition", to: "int" }),
    ).toBe("Store ordinalPosition as int");
  });

  // Most constraints have no mechanical fix, and saying so is the point. A
  // sentence that restates the violation trains a reader to ignore the column.
  it("says nothing when there is no mechanical repair", () => {
    expect(describeSuggestion(null)).toBeNull();
  });

  it("says nothing rather than a broken sentence for an unknown action", () => {
    expect(
      describeSuggestion({ action: "teleport" as Suggestion["action"], path: "1:x" }),
    ).toBeNull();
  });
});

describe("a node's display name", () => {
  it("drops the namespace code a steward did not ask for", () => {
    expect(localName("1:payments")).toBe("payments");
  });

  // A local name may itself contain a colon — `graph:reasoning` is one. Taking
  // the last segment would rename it.
  it("splits on the first colon, not the last", () => {
    expect(localName("1:graph:reasoning")).toBe("graph:reasoning");
  });

  it("leaves a bare name alone", () => {
    expect(localName("payments")).toBe("payments");
  });
});

describe("how current the report is", () => {
  // The number that makes a clean queue trustworthy: an empty queue means
  // "nothing is wrong" only if the pass ran after the last change.
  it("calls a report that ran after the last change current", () => {
    expect(currency(12, 12)).toEqual({ behind: 0, stale: false, label: "current" });
  });

  it("counts how far behind a report has fallen", () => {
    expect(currency(10, 14)).toMatchObject({ behind: 4, stale: true, label: "4 changes behind" });
  });

  // Being behind by one is being behind by one real change — the graph's `t`
  // only moves when something was written — so there is no tolerance to grant.
  it("calls out a single change rather than rounding it away", () => {
    expect(currency(10, 11)).toMatchObject({ behind: 1, stale: true, label: "1 change behind" });
  });

  // Distinct from "clean". A queue that has never been computed and one that
  // found nothing look identical, and only one of them is trustworthy.
  it("distinguishes a report that never ran from a clean one", () => {
    expect(currency(0, 5).label).toBe("never run");
    expect(currency(0, 0).stale).toBe(true);
  });

  // A report cannot be ahead of the graph. If a clock ever reads that way it
  // is a bug, and showing "-3 changes behind" would send somebody hunting it
  // in the wrong place.
  it("never reports being ahead of the graph", () => {
    expect(currency(20, 12)).toMatchObject({ behind: 0, stale: false });
  });
});

type Suggestion = NonNullable<Finding["suggestion"]>;

function waiver(over: Partial<Waiver> = {}): Waiver {
  return {
    id: "w1",
    reason: "accepted until the migration lands",
    waivedBy: "governance",
    waivedAt: "2026-07-01T00:00:00Z",
    expiresAt: "2026-12-01T00:00:00Z",
    expired: false,
    ...over,
  };
}

describe("an accepted finding", () => {
  it("is waived when a live acceptance covers it", () => {
    expect(isWaived(finding({ waiver: waiver() }))).toBe(true);
  });

  // An expired waiver accepts nothing. The record stays visible so a reader
  // can see the acceptance lapsed rather than wondering where it went.
  it("is not waived once the acceptance has lapsed", () => {
    expect(isWaived(finding({ waiver: waiver({ expired: true }) }))).toBe(false);
  });

  it("is not waived when nobody accepted it", () => {
    expect(isWaived(finding())).toBe(false);
    expect(isWaived(finding({ waiver: null }))).toBe(false);
  });

  // **Shown, not hidden.** Removing accepted work from the queue makes the
  // acceptance invisible — and with it the fact that it is about to lapse.
  it("stays in the queue", () => {
    const rows = groupByAsset([finding({ waiver: waiver() })]);

    expect(rows).toHaveLength(1);
    expect(rows[0]!.fullyWaived).toBe(true);
  });

  // But it is not what a steward works next, so it sorts below live work —
  // even when its severity is louder.
  it("sorts below unaccepted work of lower severity", () => {
    const rows = groupByAsset([
      finding({ focusNode: "1:accepted", severity: "violation", waiver: waiver() }),
      finding({ focusNode: "1:live", severity: "info" }),
    ]);

    expect(rows.map((r) => r.focusNode)).toEqual(["1:live", "1:accepted"]);
  });

  // A group is only fully accepted when *every* finding is. One unaccepted
  // problem is still a problem, and the group belongs with live work.
  it("is not fully waived when one finding is still open", () => {
    const rows = groupByAsset([
      finding({ constraint: "minCount", waiver: waiver() }),
      finding({ constraint: "maxCount" }),
    ]);

    expect(rows[0]!.fullyWaived).toBe(false);
  });

  // A group whose acceptances have all lapsed is live work again, not
  // accepted work — that is the whole reason expiry exists.
  it("returns to live work when its acceptances expire", () => {
    const rows = groupByAsset([finding({ waiver: waiver({ expired: true }) })]);

    expect(rows[0]!.fullyWaived).toBe(false);
  });
});
