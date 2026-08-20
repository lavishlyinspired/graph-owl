/** Pure logic behind the Alignment review panel — `plans/ontology-
 *  alignment-review.md`, porting Epic 104's cross-vocabulary alignment
 *  queue onto `graphowl-app` for the first time.
 *
 *  **This queue has no server-tracked status transitions.** `GET
 *  /alignments/review` always returns exactly the entries currently
 *  sitting in the 0.5–0.8 confidence review band — neither action below
 *  writes a status field; both just re-`POST` the same alignment through
 *  the server's existing confidence gate (`Catalog::upsert_alignment`).
 *
 *  **Reject has no dedicated backend route — none exists.** Posting the
 *  same alignment at confidence 0 clears it from the review band without
 *  ever asserting a graph edge: dismissed, not merely hidden client-side.
 *  There is deliberately no rejection-reason field to lose. */

import type { AlignmentReviewEntry, UpsertAlignmentRequest } from "../api";

export function formatConfidence(confidence: number | null): string {
  if (confidence === null) return "? confidence";
  return `${Math.round(confidence * 100)}% confidence`;
}

function displayTerm(term: string | null): string {
  return term ?? "(unknown)";
}

export function describeAlignment(entry: AlignmentReviewEntry): string {
  return `${displayTerm(entry.left)} ${entry.predicate ?? "?"} ${displayTerm(entry.right)}`;
}

type MatchPredicate = "exactMatch" | "closeMatch" | "broadMatch" | "narrowMatch";

/** Both actions share this shape, differing only in `source` and
 *  `confidence`. Throws if the entry is missing a field every real review
 *  entry carries together (`left`/`right`/`predicate` are always written
 *  as one unit) — a defensive read, not an expected path. */
function baseRequest(entry: AlignmentReviewEntry): Omit<UpsertAlignmentRequest, "source" | "confidence"> {
  if (!entry.left || !entry.right || !entry.predicate) {
    throw new Error("this review entry is missing left, right, or predicate — it cannot be acted on");
  }
  const isEquivalentClass = entry.predicate === "equivalentClass";
  return {
    kind: isEquivalentClass ? "equivalentClass" : "match",
    left: entry.left,
    right: entry.right,
    predicate: isEquivalentClass ? undefined : (entry.predicate as MatchPredicate),
    lossyReverse: entry.lossyReverse ?? false,
  };
}

/** A human vouching for a computed guess is a stronger claim than the
 *  guess itself — confidence 1, not the entry's own (usually sub-0.8)
 *  score, since only a value clearing the assert threshold actually
 *  writes the direct triple. */
export function confirmAlignmentRequest(entry: AlignmentReviewEntry, confirmedBy: string): UpsertAlignmentRequest {
  return {
    ...baseRequest(entry),
    source: { kind: "human", detail: confirmedBy },
    confidence: 1,
  };
}

export function rejectAlignmentRequest(entry: AlignmentReviewEntry): UpsertAlignmentRequest {
  return {
    ...baseRequest(entry),
    source: { kind: entry.sourceKind === null ? "human" : (entry.sourceKind as "curated" | "computed" | "human"), detail: entry.sourceDetail ?? "" },
    confidence: 0,
  };
}
