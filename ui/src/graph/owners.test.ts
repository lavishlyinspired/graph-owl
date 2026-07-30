import { describe, expect, it } from "vitest";
import type { EntityReference } from "../api";
import { overflowTitle, summarizeOwners } from "./owners";

function owner(
  id: string,
  kind: "user" | "team",
  displayName: string,
  inherited = false,
): EntityReference {
  return { id, kind, displayName, inherited };
}

const priya = owner("priya", "user", "Priya");
const sakshi = owner("sakshi", "user", "Sakshi");
const platform = owner("platform", "team", "Platform Team");
const data = owner("data-eng", "team", "Data Engineering");

describe("summarizing owners for the header", () => {
  // Epic 11: "an unowned asset is a real, reportable state" — not a loading
  // gap. The summary has to say so explicitly rather than just returning an
  // empty chip list, which a caller could confuse with "not loaded yet".
  it("reports no owners as unowned rather than merely empty", () => {
    const summary = summarizeOwners([]);

    expect(summary.unowned).toBe(true);
    expect(summary.chips).toEqual([]);
    expect(summary.overflow).toBe(0);
  });

  it("shows a single owner as one chip with no overflow", () => {
    const summary = summarizeOwners([priya]);

    expect(summary.unowned).toBe(false);
    expect(summary.chips).toEqual([
      { key: "priya", label: "Priya", kind: "user", inherited: false },
    ]);
    expect(summary.overflow).toBe(0);
  });

  // Users and teams side by side, in submission order — the same mixed-kind
  // case the domain's own tests assert, because the console has to render
  // exactly what Epic 11 promises rather than reinterpret it.
  it("keeps users and teams in the order the API returned them", () => {
    const summary = summarizeOwners([platform, priya]);

    expect(summary.chips.map((c) => c.key)).toEqual(["platform", "priya"]);
    expect(summary.chips[0]!.kind).toBe("team");
    expect(summary.chips[1]!.kind).toBe("user");
  });

  // The default cutoff: enough to answer "who do I ask" without the header
  // growing taller than the title above it.
  it("caps the visible chips at three by default and reports the rest as overflow", () => {
    const summary = summarizeOwners([priya, sakshi, platform, data]);

    expect(summary.chips).toHaveLength(3);
    expect(summary.overflow).toBe(1);
  });

  // The invariant a caller depends on: nothing is silently dropped, only moved
  // between the two counts.
  it("never loses an owner — chips plus overflow always equals the input length", () => {
    const owners = [priya, sakshi, platform, data];

    const summary = summarizeOwners(owners, 2);

    expect(summary.chips.length + summary.overflow).toBe(owners.length);
  });

  it("respects an explicit maxVisible", () => {
    const summary = summarizeOwners([priya, sakshi, platform], 1);

    expect(summary.chips).toHaveLength(1);
    expect(summary.overflow).toBe(2);
  });

  it("shows everything with no overflow when the list is exactly the cap", () => {
    const summary = summarizeOwners([priya, sakshi, platform], 3);

    expect(summary.chips).toHaveLength(3);
    expect(summary.overflow).toBe(0);
  });

  // A negative or zero cap must not misreport what was hidden — every owner
  // still counts as overflow rather than the function claiming nothing was cut
  // because `chips` also happened to be empty.
  it("treats a non-positive maxVisible as showing nothing, not as showing everything", () => {
    const summary = summarizeOwners([priya, sakshi], 0);

    expect(summary.chips).toEqual([]);
    expect(summary.overflow).toBe(2);
  });

  it("does not misreport overflow on a negative maxVisible either", () => {
    const summary = summarizeOwners([priya], -5);

    expect(summary.chips).toEqual([]);
    expect(summary.overflow).toBe(1);
  });

  // **The flag Slice D exists for.** Without it a chip cannot be told apart
  // from an owner recorded directly on this entity, and the whole reason to
  // walk the hierarchy — telling a steward "nobody named an owner here" from
  // "somebody did" — is lost the moment the console drops it. Both states are
  // asserted in one list so a mutant that hard-codes either `true` or `false`
  // is caught.
  it("carries whether each owner was inherited rather than recorded directly", () => {
    const direct = owner("priya", "user", "Priya", false);
    const walked = owner("platform", "team", "Platform Team", true);

    const summary = summarizeOwners([direct, walked]);

    expect(summary.chips[0]!.inherited).toBe(false);
    expect(summary.chips[1]!.inherited).toBe(true);
  });
});

// **A dashed border is not a signal anybody sees.** Verified in the browser: a
// parent's directly-recorded owners and a child's inherited ones rendered
// indistinguishably at real size, even though the flag was set correctly on
// both. Slice D's own rationale is that the flag "is the whole point of
// inheriting" — so the header needs words, not a 1px border style, and deciding
// *which* words is a decision that belongs here rather than in the component.
describe("whether the header should say the ownership is inherited", () => {
  it("says nothing when every owner was recorded on this asset", () => {
    expect(summarizeOwners([priya, platform]).inheritance).toBe("none");
  });

  it("says nothing when there are no owners at all", () => {
    expect(summarizeOwners([]).inheritance).toBe("none");
  });

  it("reports wholly inherited ownership when every owner came from above", () => {
    const walked = [
      owner("priya", "user", "Priya", true),
      owner("platform", "team", "Platform Team", true),
    ];

    expect(summarizeOwners(walked).inheritance).toBe("all");
  });

  // Mixed is a distinct answer, not a rounding of either. "Somebody named an
  // owner here *and* more came from above" is a different fact for a steward
  // than either pure case, and collapsing it would misreport whichever way it
  // rounded.
  it("reports partial inheritance when only some owners came from above", () => {
    const mixed = [priya, owner("platform", "team", "Platform Team", true)];

    expect(summarizeOwners(mixed).inheritance).toBe("some");
  });

  // The verdict is about the whole owner list, not only the chips that fit —
  // an inherited owner hidden behind "+1 more" still makes the ownership
  // partly inherited, and a header that ignored it would be wrong precisely
  // when there is too much to show.
  it("counts owners past the visible cap when deciding", () => {
    const owners = [priya, sakshi, data, owner("platform", "team", "Platform Team", true)];

    expect(summarizeOwners(owners, 3).inheritance).toBe("some");
  });

  it("is unknown-safe", () => {
    expect(summarizeOwners(undefined).inheritance).toBe("none");
  });
});

// **A server that never mentioned owners is not an asset with no owners.** The
// two look identical if `undefined` is coerced to `[]`, and the console would
// then state "no owner recorded" about an estate it was never told about — the
// same conflation this project refuses for an unmeasured semantic score or an
// untested asset. It is also not a reason to lose the whole page: the asset
// detail view is the core of the console, and crashing it over a header chip is
// the worst available outcome.
describe("a response that omits owners entirely", () => {
  it("is reported as unknown rather than as unowned", () => {
    const summary = summarizeOwners(undefined);

    expect(summary.unknown).toBe(true);
    expect(summary.unowned).toBe(false);
    expect(summary.chips).toEqual([]);
    expect(summary.overflow).toBe(0);
  });

  // The distinction, asserted from the other side so a mutant that hard-codes
  // `unknown` either way is caught.
  it("is distinguishable from an asset that genuinely has no owners", () => {
    const absent = summarizeOwners(undefined);
    const empty = summarizeOwners([]);

    expect(absent.unknown).toBe(true);
    expect(empty.unknown).toBe(false);
    expect(empty.unowned).toBe(true);
  });

  it("does not report unknown for an asset that has owners", () => {
    expect(summarizeOwners([priya]).unknown).toBe(false);
  });

  it("does not throw when building the overflow tooltip either", () => {
    expect(overflowTitle(undefined)).toBe("");
  });
});

describe("the overflow tooltip", () => {
  it("lists exactly the owners that were cut, not the visible ones", () => {
    const title = overflowTitle([priya, sakshi, platform, data], 2);

    expect(title).toBe("Platform Team, Data Engineering");
  });

  it("is empty when nothing overflowed", () => {
    expect(overflowTitle([priya], 3)).toBe("");
  });

  it("is empty for an empty owner list", () => {
    expect(overflowTitle([], 3)).toBe("");
  });
});
