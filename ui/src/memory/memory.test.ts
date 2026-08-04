import { describe, expect, it } from "vitest";
import {
  IGNORE_BAND,
  type Memory,
  type RecalledMemory,
  authorLabel,
  isVisibleOnEntityPage,
  stalenessLabel,
} from "./memory";

function memory(overrides?: Partial<Memory>): Memory {
  return {
    id: "1",
    kind: "decision",
    content: "Refunds are included.",
    summary: null,
    authorship: { kind: "human", userId: "priya" },
    confidence: 0.9,
    links: [],
    asOf: "2026-01-01T00:00:00Z",
    supersedes: null,
    supersededBy: null,
    retractedAt: null,
    retractionReason: null,
    ...overrides,
  };
}

function recalled(overrides?: Partial<RecalledMemory>): RecalledMemory {
  return {
    memory: memory(),
    staleness: { state: "fresh" },
    score: {
      anchor: 1,
      lexical: 0,
      semantic: null,
      staleness: 0,
      recency: 0.5,
      authorship: 0.2,
      confidence: 0.3,
      total: 2,
    },
    ...overrides,
  };
}

describe("the ignore band decides what surfaces on the entity page", () => {
  // The plan's own boundary: "a memory below the ignore band (<0.5) is not
  // surfaced on the entity page but is findable in administration."
  it("hides a memory below the band", () => {
    expect(isVisibleOnEntityPage(recalled({ memory: memory({ confidence: 0.49 }) }))).toBe(
      false,
    );
  });

  it("shows a memory exactly at the band", () => {
    expect(isVisibleOnEntityPage(recalled({ memory: memory({ confidence: IGNORE_BAND }) }))).toBe(
      true,
    );
  });

  it("shows a memory above the band", () => {
    expect(isVisibleOnEntityPage(recalled({ memory: memory({ confidence: 0.9 }) }))).toBe(true);
  });

  // A retracted memory is never surfaced on the entity page regardless of
  // confidence — it is no longer believed, which is a stronger statement
  // than "not sure enough to show".
  it("hides a retracted memory even at full confidence", () => {
    expect(
      isVisibleOnEntityPage(
        recalled({
          memory: memory({ confidence: 1, retractedAt: "2026-02-01T00:00:00Z" }),
        }),
      ),
    ).toBe(false);
  });
});

describe("staleness reads as a short label", () => {
  it("labels a fresh memory", () => {
    expect(stalenessLabel({ state: "fresh" })).toBe("Fresh");
  });

  it("labels a possibly-stale memory", () => {
    expect(
      stalenessLabel({ state: "possiblyStale", since: { major: 1, minor: 2 } }),
    ).toBe("Possibly stale");
  });

  it("labels a stale memory", () => {
    expect(stalenessLabel({ state: "stale", since: { major: 2, minor: 0 } })).toBe("Stale");
  });

  it("labels an unresolvable subject", () => {
    expect(stalenessLabel({ state: "subjectUnknown" })).toBe("Subject unknown");
  });
});

describe("authorship reads as who or what wrote it", () => {
  it("names a human author", () => {
    expect(authorLabel({ kind: "human", userId: "priya" })).toBe("priya");
  });

  it("names an agent author with its model", () => {
    expect(authorLabel({ kind: "agent", agentId: "lineage-bot", model: "gpt-5" })).toBe(
      "lineage-bot (gpt-5)",
    );
  });
});
