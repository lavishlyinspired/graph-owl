import { describe, expect, it } from "vitest";
import { describeReasoningRun } from "./reasoningRun";

const base = {
  technique: "full" as const,
  derived: 3,
  replaced: 0,
  maintainedTo: 12,
  partial: false,
  ignoredAxioms: [],
};

describe("describeReasoningRun", () => {
  it("names a full run's technique and counts", () => {
    expect(describeReasoningRun(base).headline).toBe(
      "Full run — 3 derived, 0 replaced, maintained to t=12",
    );
  });

  it("names an incremental run distinctly from a full one", () => {
    expect(describeReasoningRun({ ...base, technique: "incremental" }).headline).toBe(
      "Incremental run — 3 derived, 0 replaced, maintained to t=12",
    );
  });

  it("reports the watermark, not a fabricated age", () => {
    expect(describeReasoningRun({ ...base, maintainedTo: 999 }).headline).toContain("t=999");
  });

  it("says nothing when nothing was ignored", () => {
    expect(describeReasoningRun(base).warning).toBeNull();
  });

  it("warns, naming the ignored count, when the run was partial", () => {
    const run = {
      ...base,
      partial: true,
      ignoredAxioms: [
        { subject: "1:Widget", reason: "hasKey" },
        { subject: "1:Gadget", reason: "propertyChain" },
      ],
    };
    expect(describeReasoningRun(run).warning).toBe(
      "Partial: 2 axioms outside this profile were ignored to produce this result.",
    );
  });

  it("singularizes both the noun and the verb for one ignored axiom", () => {
    const run = { ...base, partial: true, ignoredAxioms: [{ subject: "1:Widget", reason: "hasKey" }] };
    expect(describeReasoningRun(run).warning).toBe(
      "Partial: 1 axiom outside this profile was ignored to produce this result.",
    );
  });
});
