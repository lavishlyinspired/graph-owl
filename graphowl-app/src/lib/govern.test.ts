import { describe, expect, it } from "vitest";
import {
  deniesEverything,
  draftIncomplete,
  draftToPolicy,
  evidenceSummary,
  validationKpis,
  type PolicyDraft,
} from "./govern";
import type { DryRunOutcome, Evidence, ValidationFinding } from "./api";

describe("validationKpis — Plan 122a A5", () => {
  const findings: readonly ValidationFinding[] = [
    {
      id: "f1",
      shape: "shape:Supplier",
      focusNode: "supplier-1",
      path: null,
      constraint: "sh:minCount",
      severity: "violation",
      message: "missing owner",
      actual: null,
      suggestion: null,
      assignment: null,
      waiver: null,
    },
    {
      id: "f2",
      shape: "shape:Supplier",
      focusNode: "supplier-1",
      path: null,
      constraint: "sh:pattern",
      severity: "warning",
      message: "GSTIN format",
      actual: null,
      suggestion: null,
      assignment: null,
      waiver: null,
    },
    {
      id: "f3",
      shape: "shape:Invoice",
      focusNode: "invoice-9",
      path: null,
      constraint: "sh:minCount",
      severity: "violation",
      message: "missing filing period",
      actual: null,
      suggestion: null,
      assignment: null,
      waiver: null,
    },
  ];

  it("counts violations and warnings separately, not a combined total", () => {
    const kpis = validationKpis(findings);
    expect(kpis.violations).toBe(2);
    expect(kpis.warnings).toBe(1);
  });

  /** Mutator: counting rows rather than distinct focus nodes would report 3
   *  affected here, since two findings share `supplier-1`. */
  it("counts distinct affected focus nodes, not distinct findings", () => {
    const kpis = validationKpis(findings);
    expect(kpis.affected).toBe(2);
  });

  it("is all zero, not broken, for an empty report", () => {
    expect(validationKpis([])).toEqual({ violations: 0, warnings: 0, affected: 0 });
  });
});

describe("evidenceSummary — Plan 122a A5", () => {
  it("names an exact FQN match in plain words", () => {
    const evidence: readonly Evidence[] = [{ kind: "exactFqn" }];
    expect(evidenceSummary(evidence)).toBe("exact FQN match");
  });

  it("includes the metric and score for a name-similarity match", () => {
    const evidence: readonly Evidence[] = [{ kind: "nameSimilarity", metric: "jaro-winkler", value: 0.94 }];
    expect(evidenceSummary(evidence)).toBe("jaro-winkler name similarity 0.94");
  });

  it("includes the actual shared/total counts for structural overlap, not a fixed phrase", () => {
    const evidence: readonly Evidence[] = [{ kind: "structuralOverlap", sharedColumns: 3, total: 5 }];
    expect(evidenceSummary(evidence)).toBe("3/5 shared columns");
  });

  it("joins multiple evidence entries into one readable summary", () => {
    const evidence: readonly Evidence[] = [{ kind: "sameParent" }, { kind: "sameSourceSystem" }];
    expect(evidenceSummary(evidence)).toBe("same parent, same source system");
  });

  it("is a real placeholder, not blank, when there is no evidence", () => {
    expect(evidenceSummary([])).toBe("no recorded evidence");
  });
});

describe("draftIncomplete — Plan 122a A5", () => {
  const validDraft: PolicyDraft = {
    name: "reader-policy",
    roles: ["analyst"],
    rules: [
      { name: "allow-reads", effect: "allow", operations: ["viewBasic"], resourceType: "all", resourceValue: "" },
    ],
  };

  it("is complete when the policy has a name and at least one valid rule", () => {
    expect(draftIncomplete(validDraft)).toBe(false);
  });

  it("is incomplete when the policy name is blank", () => {
    expect(draftIncomplete({ ...validDraft, name: "  " })).toBe(true);
  });

  it("is incomplete when there are no rules", () => {
    expect(draftIncomplete({ ...validDraft, rules: [] })).toBe(true);
  });

  it("is incomplete when a rule has no operations selected", () => {
    expect(draftIncomplete({ ...validDraft, rules: [{ ...validDraft.rules[0]!, operations: [] }] })).toBe(true);
  });

  /** Mutator: checking only the first rule would let a second, broken rule
   *  through. */
  it("is incomplete when a later rule (not the first) is missing a name", () => {
    const secondRuleBlank: PolicyDraft = {
      ...validDraft,
      rules: [validDraft.rules[0]!, { ...validDraft.rules[0]!, name: "" }],
    };
    expect(draftIncomplete(secondRuleBlank)).toBe(true);
  });

  it("is incomplete when a non-'all' resource matcher has no value", () => {
    const noValue: PolicyDraft = {
      ...validDraft,
      rules: [{ ...validDraft.rules[0]!, resourceType: "fqnPrefix", resourceValue: "" }],
    };
    expect(draftIncomplete(noValue)).toBe(true);
  });
});

describe("draftToPolicy — Plan 122a A5", () => {
  it("carries the 'all' resource matcher with no value field", () => {
    const draft: PolicyDraft = {
      name: "p",
      roles: [],
      rules: [{ name: "r", effect: "allow", operations: ["viewBasic"], resourceType: "all", resourceValue: "" }],
    };
    expect(draftToPolicy(draft).rules[0]?.resources).toEqual({ type: "all" });
  });

  it("carries the resource value for a non-'all' matcher, not just its type", () => {
    const draft: PolicyDraft = {
      name: "p",
      roles: [],
      rules: [
        { name: "r", effect: "deny", operations: ["delete"], resourceType: "fqnPrefix", resourceValue: "gst." },
      ],
    };
    expect(draftToPolicy(draft).rules[0]?.resources).toEqual({ type: "fqnPrefix", value: "gst." });
  });
});

describe("deniesEverything — Plan 122a A5", () => {
  const base: DryRunOutcome = { admitted: 0, denied: 0, total: 0, examples: [], admitsEverything: false };

  it("is true when nothing in a non-empty estate was admitted", () => {
    expect(deniesEverything({ ...base, admitted: 0, denied: 10, total: 10 })).toBe(true);
  });

  it("is false when at least one asset was admitted", () => {
    expect(deniesEverything({ ...base, admitted: 1, denied: 9, total: 10 })).toBe(false);
  });

  it("is false for an empty estate, not vacuously true", () => {
    expect(deniesEverything({ ...base, admitted: 0, denied: 0, total: 0 })).toBe(false);
  });
});
