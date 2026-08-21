/** Pure decision logic for the GOVERN group (Plan 122a A5: Validation,
 *  Resolution, Drift, Governance) — kept separate from the route components
 *  so it can be unit-tested without rendering anything, same split as
 *  `trace.ts`. Contradictions' own pure logic — filtering, sorting and
 *  labelling the `/findings` queue — lives in `lib/findingsQueue.ts`
 *  instead: it is about a different API entirely (findings, not policies or
 *  drift), and grouping it here would make this file's name a lie. */

import type { DryRunOutcome, Evidence, MetadataOperation, Policy, PolicyEffect, ResourceMatcher, ValidationFinding } from "./api";

export interface ValidationKpis {
  readonly violations: number;
  readonly warnings: number;
  readonly affected: number;
}

export function validationKpis(findings: readonly ValidationFinding[]): ValidationKpis {
  return {
    violations: findings.filter((f) => f.severity === "violation").length,
    warnings: findings.filter((f) => f.severity === "warning").length,
    affected: new Set(findings.map((f) => f.focusNode)).size,
  };
}

function oneEvidenceSummary(evidence: Evidence): string {
  switch (evidence.kind) {
    case "exactFqn":
      return "exact FQN match";
    case "normalizedFqn":
      return "normalized FQN match";
    case "exactName":
      return `exact name match within ${evidence.scope}`;
    case "nameSimilarity":
      return `${evidence.metric} name similarity ${evidence.value}`;
    case "structuralOverlap":
      return `${evidence.sharedColumns}/${evidence.total} shared columns`;
    case "sameParent":
      return "same parent";
    case "sameSourceSystem":
      return "same source system";
  }
}

export function evidenceSummary(evidence: readonly Evidence[]): string {
  if (evidence.length === 0) return "no recorded evidence";
  return evidence.map(oneEvidenceSummary).join(", ");
}

export interface PolicyDraftRule {
  readonly name: string;
  readonly effect: PolicyEffect;
  readonly operations: readonly MetadataOperation[];
  readonly resourceType: ResourceMatcher["type"];
  readonly resourceValue: string;
}

export interface PolicyDraft {
  readonly name: string;
  readonly roles: readonly string[];
  readonly rules: readonly PolicyDraftRule[];
}

function ruleIncomplete(rule: PolicyDraftRule): boolean {
  if (rule.name.trim().length === 0) return true;
  if (rule.operations.length === 0) return true;
  if (rule.resourceType !== "all" && rule.resourceValue.trim().length === 0) return true;
  return false;
}

export function draftIncomplete(draft: PolicyDraft): boolean {
  if (draft.name.trim().length === 0) return true;
  if (draft.rules.length === 0) return true;
  return draft.rules.some(ruleIncomplete);
}

function ruleResources(rule: PolicyDraftRule): ResourceMatcher {
  if (rule.resourceType === "all") return { type: "all" };
  return { type: rule.resourceType, value: rule.resourceValue };
}

export function draftToPolicy(draft: PolicyDraft): Policy {
  return {
    name: draft.name,
    rules: draft.rules.map((rule) => ({
      name: rule.name,
      effect: rule.effect,
      operations: rule.operations,
      resources: ruleResources(rule),
    })),
  };
}

/** The server computes `admitsEverything`; `deniesEverything` has no server
 *  equivalent (`plans/122a`'s GOVERN research), so it is derived here the
 *  same way — real counts, not a fabricated field. */
export function deniesEverything(outcome: DryRunOutcome): boolean {
  return outcome.admitted === 0 && outcome.total > 0;
}
