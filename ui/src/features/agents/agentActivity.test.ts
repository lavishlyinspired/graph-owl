import { describe, expect, it } from "vitest";
import type { AgentActivity } from "../../api";
import { describeCapability, describeOutcome, filterActivity } from "./agentActivity";

function activity(overrides: Partial<AgentActivity> = {}): AgentActivity {
  return {
    id: "a1",
    agentId: "agent-alpha",
    capability: "applyDescription",
    targetFqn: "warehouse.orders",
    outcome: "applied",
    at: "2026-08-07T00:00:00Z",
    ...overrides,
  };
}

describe("describeOutcome", () => {
  it("marks applied as a write-back, linked to what it changed", () => {
    const result = describeOutcome(activity({ outcome: "applied", targetFqn: "warehouse.orders" }));
    expect(result.label).toBe("Applied");
    expect(result.isWriteBack).toBe(true);
    expect(result.detail).toContain("warehouse.orders");
  });

  it("marks proposed as NOT a write-back — the RED test's own distinction", () => {
    const result = describeOutcome(activity({ outcome: "proposed", targetFqn: "warehouse.orders" }));
    expect(result.label).toBe("Proposed");
    expect(result.isWriteBack).toBe(false);
    expect(result.detail).toContain("warehouse.orders");
  });

  it("marks refused as NOT a write-back, and surfaces the refusal reason when present", () => {
    const result = describeOutcome(
      activity({ outcome: "refused", refusal: "missing capability applyTags" }),
    );
    expect(result.label).toBe("Refused");
    expect(result.isWriteBack).toBe(false);
    expect(result.detail).toContain("missing capability applyTags");
  });

  it("a refusal with no reason still names the target, never a blank detail", () => {
    const result = describeOutcome(activity({ outcome: "refused", refusal: undefined, targetFqn: "vault" }));
    expect(result.detail.length).toBeGreaterThan(0);
    expect(result.detail).toContain("vault");
  });
});

describe("describeCapability", () => {
  it("gives every capability a distinct, human-readable label", () => {
    const capabilities: Array<AgentActivity["capability"]> = [
      "proposeDescription",
      "proposeTags",
      "proposeOwner",
      "applyDescription",
      "applyTags",
      "recordMemory",
      "recordInvestigation",
      "createGlossaryTerm",
      "createQualityTest",
      "linkLineage",
    ];
    const labels = capabilities.map(describeCapability);
    expect(new Set(labels).size).toBe(capabilities.length);
    expect(labels.every((l) => l.length > 0)).toBe(true);
  });
});

describe("filterActivity", () => {
  const entries = [
    activity({ id: "1", capability: "applyDescription", outcome: "applied", targetFqn: "warehouse.orders" }),
    activity({ id: "2", capability: "proposeTags", outcome: "proposed", targetFqn: "warehouse.customers" }),
    activity({ id: "3", capability: "applyDescription", outcome: "refused", targetFqn: "vault.secrets" }),
  ];

  it("with no filter, returns everything", () => {
    expect(filterActivity(entries, {})).toHaveLength(3);
  });

  it("filters by type (capability)", () => {
    const result = filterActivity(entries, { capability: "applyDescription" });
    expect(result.map((e) => e.id)).toEqual(["1", "3"]);
  });

  it("filters by outcome", () => {
    const result = filterActivity(entries, { outcome: "refused" });
    expect(result.map((e) => e.id)).toEqual(["3"]);
  });

  it("filters by entity (substring match against targetFqn, case-insensitive)", () => {
    const result = filterActivity(entries, { entity: "WAREHOUSE" });
    expect(result.map((e) => e.id)).toEqual(["1", "2"]);
  });

  it("combines all three filters", () => {
    const result = filterActivity(entries, {
      capability: "applyDescription",
      outcome: "applied",
      entity: "orders",
    });
    expect(result.map((e) => e.id)).toEqual(["1"]);
  });

  it("an entity filter matching nothing returns an empty list, not everything", () => {
    expect(filterActivity(entries, { entity: "no-such-entity" })).toEqual([]);
  });
});
