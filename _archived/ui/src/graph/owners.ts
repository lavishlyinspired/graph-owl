/** Summarizing who owns an asset — Epic 39, the console half of Epic 11 Slice C.
 *
 *  `00c-domain-model.md`: "single-owner models fail immediately — every real
 *  asset has a producing team and an accountable individual." The API already
 *  returns a denormalized, ordered `owners` list; what can go wrong here is
 *  entirely in *display*: how many chips to show before a header becomes a
 *  wall of names, and what "unowned" looks like when it is a real state rather
 *  than a loading gap.
 *
 *  `00f-ui-architecture.md`: the part that can be wrong in a way somebody would
 *  act on belongs in a pure function, tested without rendering anything. */

import type { EntityReference } from "../api";

export interface OwnerChip {
  readonly key: string;
  readonly label: string;
  readonly kind: "user" | "team";
  /** Found by walking the containment hierarchy rather than recorded here —
   *  see [`EntityReference.inherited`]. Carried through unchanged: the console
   *  is not the place that decides what counts as inherited, only the place
   *  that has to say so when it is. */
  readonly inherited: boolean;
}

/** How much of this asset's ownership came from an ancestor rather than being
 *  recorded on the asset itself.
 *
 *  Three values, because `some` is a distinct fact rather than a rounding of
 *  either pure case: "somebody named an owner here *and* more came from above"
 *  is different from both for a steward deciding whether ownership has actually
 *  been thought about at this level. */
export type Inheritance = "none" | "some" | "all";

export interface OwnerSummary {
  /** Whether to tell the reader the ownership is inherited, and how much of it.
   *
   *  Decided over the **whole** owner list, not only the chips that fit: an
   *  inherited owner hidden behind "+1 more" still makes the ownership partly
   *  inherited, and a header that ignored it would be wrong exactly when there
   *  is too much to show. */
  readonly inheritance: Inheritance;
  /** The response did not carry an `owners` field at all — an older server, or a
   *  read that never included it. **Not the same as `unowned`**, and kept apart
   *  for the same reason this codebase keeps an unmeasured semantic score apart
   *  from a zero one: saying "no owner recorded" about an estate nobody asked
   *  about is a claim, not a blank. Callers should render nothing rather than
   *  assert either way. */
  readonly unknown: boolean;
  /** Owners to render as chips, in the order the domain returned them —
   *  submission order is a correctness signal there (validation errors are
   *  reported by index), and a display that reordered them would make a
   *  screenshot disagree with the API response sitting beside it. */
  readonly chips: readonly OwnerChip[];
  /** How many more owners exist beyond `chips`. Zero when nothing was cut. */
  readonly overflow: number;
  /** True when the asset genuinely has no owners — a real, reportable state
   *  per Epic 11, not the same thing as "not loaded yet". Callers that have not
   *  loaded owners yet should not call this with `[]`; they should not render
   *  the header piece at all until they know. */
  readonly unowned: boolean;
}

/** Turn a raw owner list into what the header actually shows.
 *
 *  `maxVisible` defaults to 3: enough to answer "who do I ask" without a
 *  header that grows taller than the title above it on an asset with a dozen
 *  owners. Below zero would drop every chip and still claim nothing was
 *  hidden if `owners` were also empty — `Math.max(0, …)` keeps the invariant
 *  `chips.length + overflow === owners.length` unconditionally. */
export function summarizeOwners(
  // `undefined` is accepted deliberately, even though the server's contract says
  // the field is always present. A type is a compile-time claim about a *build*,
  // not a runtime guarantee about whichever server is actually answering — and
  // when this was typed as required, an older server omitting the field took the
  // entire asset page down with `Cannot read properties of undefined`. Types
  // cannot enforce anything across an HTTP boundary.
  owners: readonly EntityReference[] | undefined,
  maxVisible = 3,
): OwnerSummary {
  if (owners === undefined) {
    return { chips: [], overflow: 0, unowned: false, unknown: true, inheritance: "none" };
  }
  const visible = Math.max(0, maxVisible);
  const chips = owners.slice(0, visible).map((owner) => ({
    key: owner.id,
    label: owner.displayName,
    kind: owner.kind,
    inherited: owner.inherited,
  }));
  const inheritedCount = owners.filter((owner) => owner.inherited).length;
  return {
    chips,
    overflow: owners.length - chips.length,
    unowned: owners.length === 0,
    unknown: false,
    // `owners.length > 0` guards the empty case: `every` is vacuously true on an
    // empty array, which would report an unowned asset as wholly inherited.
    inheritance:
      inheritedCount === 0
        ? "none"
        : inheritedCount === owners.length
          ? "all"
          : "some",
  };
}

/** The tooltip/title text for the overflow indicator — every hidden owner's
 *  name, so "+2 more" is not a dead end for someone who wants to know who. */
export function overflowTitle(
  owners: readonly EntityReference[] | undefined,
  maxVisible = 3,
): string {
  if (owners === undefined) return "";
  const visible = Math.max(0, maxVisible);
  return owners
    .slice(visible)
    .map((owner) => owner.displayName)
    .join(", ");
}
