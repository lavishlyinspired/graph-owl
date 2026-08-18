/** Epic 42 Slice F: pure decision logic for the Agent activity admin tab.
 *
 *  **This module's own named RED test**: "write-backs distinguished from
 *  reads and linked to what they changed." Epic 32's `AgentActivity` has no
 *  "read" outcome — every entry is a write attempt (`applied`, `proposed`,
 *  or `refused`) — so the distinction that matters here is the one the data
 *  actually carries: `applied` is a **write-back that changed the catalog**,
 *  `proposed` and `refused` are not. `describeOutcome` must never blur that
 *  line into one generic "activity happened" label, and must always name
 *  `targetFqn` — an entry that does not say what it touched is not "linked
 *  to what it changed". */

import type { AgentActivity, AgentCapability } from "../../api";

export interface OutcomeDescription {
  readonly label: string;
  /** The one true write-back signal: the catalog actually changed. */
  readonly isWriteBack: boolean;
  readonly detail: string;
}

export function describeOutcome(activity: AgentActivity): OutcomeDescription {
  switch (activity.outcome) {
    case "applied":
      return {
        label: "Applied",
        isWriteBack: true,
        detail: `Wrote directly to ${activity.targetFqn}.`,
      };
    case "proposed":
      return {
        label: "Proposed",
        isWriteBack: false,
        detail: `Suggested a change to ${activity.targetFqn} — pending human review.`,
      };
    case "refused":
      return {
        label: "Refused",
        isWriteBack: false,
        detail: activity.refusal
          ? `Refused: ${activity.refusal}`
          : `Refused on ${activity.targetFqn}.`,
      };
  }
}

const CAPABILITY_LABELS: Record<AgentCapability, string> = {
  proposeDescription: "Propose description",
  proposeTags: "Propose tags",
  proposeOwner: "Propose owner",
  applyDescription: "Apply description",
  applyTags: "Apply tags",
  recordMemory: "Record memory",
  recordInvestigation: "Record investigation",
  createGlossaryTerm: "Create glossary term",
  createQualityTest: "Create quality test",
  linkLineage: "Link lineage",
};

export function describeCapability(capability: AgentCapability): string {
  return CAPABILITY_LABELS[capability];
}

export interface ActivityFilter {
  readonly capability?: AgentCapability | null;
  readonly outcome?: AgentActivity["outcome"] | null;
  /** Substring match against `targetFqn` — "filterable by ... entity". */
  readonly entity?: string | null;
}

/** Pure so both the panel and its own tests can prove "type" and "entity"
 *  filtering independently, and combined, without a live server. */
export function filterActivity(
  activities: readonly AgentActivity[],
  filter: ActivityFilter,
): AgentActivity[] {
  const entity = filter.entity?.trim().toLowerCase();
  return activities.filter((activity) => {
    if (filter.capability && activity.capability !== filter.capability) return false;
    if (filter.outcome && activity.outcome !== filter.outcome) return false;
    if (entity && !activity.targetFqn.toLowerCase().includes(entity)) return false;
    return true;
  });
}
