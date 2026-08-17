import { describe, expect, it } from "vitest";
import { kindLabel, pairsFor } from "./contradictions";
import type { Contradiction, Memory } from "../api";

const memory = (id: string, content: string): Memory => ({
  id,
  kind: "decision",
  content,
  summary: null,
  authorship: { kind: "human", userId: "asha" },
  confidence: 0.9,
  links: [],
  asOf: "2026-08-01T00:00:00Z",
  supersedes: null,
  supersededBy: null,
  retractedAt: null,
  retractionReason: null,
});

const pair = (overrides?: Partial<Contradiction>): Contradiction => ({
  a: "m1",
  b: "m2",
  subject: "asset-1",
  kind: "candidate",
  ...overrides,
});

describe("pairing a flagged contradiction with the memories it names", () => {
  it("resolves both sides when both are in hand", () => {
    const [resolved] = pairsFor(
      [pair()],
      [memory("m1", "we keep this"), memory("m2", "we drop this")],
    );
    expect(resolved!.a?.content).toBe("we keep this");
    expect(resolved!.b?.content).toBe("we drop this");
    expect(resolved!.complete).toBe(true);
  });

  /** **A disagreement with one side missing is still a disagreement.**
   *  Recall filters by confidence, so the other memory may be real and simply
   *  not shown here — dropping the whole pair would hide the fact that
   *  something conflicts, which is the one thing this surface exists to say. */
  it("keeps a pair whose other side was not recalled, and marks it incomplete", () => {
    const [resolved] = pairsFor([pair()], [memory("m1", "we keep this")]);
    expect(resolved!.a?.content).toBe("we keep this");
    expect(resolved!.b).toBeNull();
    expect(resolved!.complete).toBe(false);
  });

  it("keeps a pair with neither side recalled rather than silently discarding it", () => {
    expect(pairsFor([pair()], [])).toHaveLength(1);
  });

  it("preserves the pair's own identity so a verdict can be recorded against it", () => {
    const [resolved] = pairsFor([pair({ a: "x", b: "y" })], []);
    expect([resolved!.id.a, resolved!.id.b]).toEqual(["x", "y"]);
  });
});

describe("saying why a pair was flagged", () => {
  /** **"A person said so" and "a rule guessed" must not read alike.** A
   *  reviewer's next action differs entirely: one is a fact to act on, the
   *  other is a candidate to judge. */
  it("distinguishes a human assertion from a heuristic candidate", () => {
    expect(kindLabel("declared")).not.toBe(kindLabel("candidate"));
    expect(kindLabel("candidate").toLowerCase()).toContain("might");
    expect(kindLabel("declared").toLowerCase()).toContain("someone");
  });

  /** Confirmed is not resolved, and the wording must not suggest it is —
   *  the engine deliberately keeps a confirmed pair in the queue. */
  it("says a confirmed pair is agreed to disagree, not settled", () => {
    expect(kindLabel("confirmed").toLowerCase()).not.toContain("resolved");
  });
});
