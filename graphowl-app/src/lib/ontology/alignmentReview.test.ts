import { describe, expect, it } from "vitest";
import { confirmAlignmentRequest, describeAlignment, formatConfidence, rejectAlignmentRequest } from "./alignmentReview";
import type { AlignmentReviewEntry } from "../api";

function entry(overrides?: Partial<AlignmentReviewEntry>): AlignmentReviewEntry {
  return {
    subject: "1:alignment-abc",
    left: "snomed:22298006",
    right: "icd10:I21",
    predicate: "closeMatch",
    sourceKind: "computed",
    sourceDetail: "string-similarity",
    confidence: 0.62,
    lossyReverse: false,
    ...overrides,
  };
}

describe("formatting an alignment's confidence", () => {
  it("renders a fraction as a rounded percentage", () => {
    expect(formatConfidence(0.62)).toBe("62% confidence");
  });

  it("rounds to the nearest whole percent", () => {
    expect(formatConfidence(0.5049)).toBe("50% confidence");
  });

  it("says so when no confidence was recorded", () => {
    expect(formatConfidence(null)).toBe("? confidence");
  });
});

describe("describing what an alignment claims", () => {
  it("names left, predicate and right in order", () => {
    expect(describeAlignment(entry())).toBe("snomed:22298006 closeMatch icd10:I21");
  });

  it("marks a missing term as unknown rather than blank", () => {
    expect(describeAlignment(entry({ left: null }))).toBe("(unknown) closeMatch icd10:I21");
  });

  it("marks a missing predicate with a bare question mark", () => {
    expect(describeAlignment(entry({ predicate: null }))).toBe("snomed:22298006 ? icd10:I21");
  });
});

describe("building the Confirm request", () => {
  it("asserts full confidence, attributed to the confirming human", () => {
    const request = confirmAlignmentRequest(entry(), "akash");
    expect(request).toEqual({
      kind: "match",
      left: "snomed:22298006",
      right: "icd10:I21",
      predicate: "closeMatch",
      source: { kind: "human", detail: "akash" },
      confidence: 1,
      lossyReverse: false,
    });
  });

  /** `owl:equivalentClass` carries logical force, so it is never sent as
   *  the `predicate` field — `kind` alone is what names it. */
  it("switches to kind: equivalentClass and drops the predicate field", () => {
    const request = confirmAlignmentRequest(entry({ predicate: "equivalentClass" }), "akash");
    expect(request).toMatchObject({ kind: "equivalentClass" });
    expect(request.predicate).toBeUndefined();
  });

  it("refuses an entry missing left, right, or predicate — there is nothing to confirm", () => {
    expect(() => confirmAlignmentRequest(entry({ left: null }), "akash")).toThrow();
  });
});

describe("building the Reject request", () => {
  /** No dedicated reject route exists — decision recorded when this was
   *  ported: re-posting at confidence 0 clears the review-band metadata
   *  without ever asserting a graph edge. */
  it("re-posts the same alignment at zero confidence, under its own original source", () => {
    const request = rejectAlignmentRequest(entry());
    expect(request).toEqual({
      kind: "match",
      left: "snomed:22298006",
      right: "icd10:I21",
      predicate: "closeMatch",
      source: { kind: "computed", detail: "string-similarity" },
      confidence: 0,
      lossyReverse: false,
    });
  });

  it("falls back to a human source when the original source is unrecorded", () => {
    const request = rejectAlignmentRequest(entry({ sourceKind: null, sourceDetail: null }));
    expect(request.source).toEqual({ kind: "human", detail: "" });
  });
});
