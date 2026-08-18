/** Organizational memory on the console — Epic 41 Slice E.
 *
 *  Entity-scoped recall lives on the asset page; cross-entity search and
 *  retraction live in Admin. Both read the same wire shapes, defined once
 *  here so the two screens cannot quietly disagree about what a `Memory` is. */

export type Authorship =
  | { readonly kind: "human"; readonly userId: string }
  | { readonly kind: "agent"; readonly agentId: string; readonly model: string };

export type LinkRelation = "about" | "affects" | "evidence" | "follows" | "contradicts" | "mentions";

export interface MemoryLink {
  readonly relation: LinkRelation;
  readonly target: string;
}

export interface EntityVersion {
  readonly major: number;
  readonly minor: number;
}

export type Staleness =
  | { readonly state: "fresh" }
  | { readonly state: "possiblyStale"; readonly since: EntityVersion }
  | { readonly state: "stale"; readonly since: EntityVersion }
  | { readonly state: "subjectUnknown" };

export interface Memory {
  readonly id: string;
  readonly kind: "rationale" | "incident" | "decision" | "caveat";
  readonly content: string;
  readonly summary: string | null;
  readonly authorship: Authorship;
  readonly confidence: number;
  readonly links: readonly MemoryLink[];
  readonly asOf: string;
  readonly supersedes: string | null;
  readonly supersededBy: string | null;
  readonly retractedAt: string | null;
  readonly retractionReason: string | null;
}

export interface Score {
  readonly anchor: number;
  readonly lexical: number;
  readonly semantic: number | null;
  readonly staleness: number;
  readonly recency: number;
  readonly authorship: number;
  readonly confidence: number;
  readonly total: number;
}

export interface RecalledMemory {
  readonly memory: Memory;
  readonly staleness: Staleness;
  readonly score: Score;
}

/** Below this, a memory is not surfaced on the entity page — the plan's own
 *  stated boundary. Still findable in administration, which is the whole
 *  point of keeping it rather than deleting it. */
export const IGNORE_BAND = 0.5;

/** Whether a recalled memory belongs on the entity page.
 *
 *  Two independent reasons to hide one: too little confidence to be worth
 *  reading by default, or retracted — which is a stronger statement than
 *  low confidence, since a retracted memory is not believed at all
 *  regardless of how sure the author originally was. */
export function isVisibleOnEntityPage(recalled: RecalledMemory): boolean {
  if (recalled.memory.retractedAt !== null) return false;
  return recalled.memory.confidence >= IGNORE_BAND;
}

/** A short label for the staleness verdict. */
export function stalenessLabel(staleness: Staleness): string {
  switch (staleness.state) {
    case "fresh":
      return "Fresh";
    case "possiblyStale":
      return "Possibly stale";
    case "stale":
      return "Stale";
    case "subjectUnknown":
      return "Subject unknown";
  }
}

/** Who or what wrote a memory, as a reader reads it. */
export function authorLabel(authorship: Authorship): string {
  return authorship.kind === "human"
    ? authorship.userId
    : `${authorship.agentId} (${authorship.model})`;
}
