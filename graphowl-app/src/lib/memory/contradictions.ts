/** Open disagreements between two things this deployment believes —
 *  Plan 111 Slice C.
 *
 *  **`GET /assets/{id}/contradictions` and `POST /contradictions/reviews`
 *  shipped with Epic 31 and no console has ever called either.** A product
 *  whose whole claim is that conflicting institutional knowledge becomes
 *  *visible* rather than silently inconsistent was keeping the conflicts
 *  where only an integrator could find them.
 *
 *  **This is a graph-native finding and could not exist without the
 *  architecture.** Two sources asserting incompatible things about one
 *  subject needs named graphs and provenance to be expressible at all; a row
 *  store has one value and no memory of the other.
 *
 *  **Nothing here resolves anything.** The engine never picks a winner and
 *  never hides either side — confirming a pair leaves it in the queue,
 *  flagged, because software that adjudicates institutional disagreement ends
 *  the argument without settling it. The wording below is chosen so the
 *  screen cannot imply otherwise.
 *
 *  Domain-agnostic: a memory is an attributed claim about an entity, and what
 *  the claims are *about* is the deployment's business.
 *
 *  Ported from `ui/src/memory/contradictions.ts` (Plan 122a A3) — pure
 *  logic, unchanged. */

import type { Contradiction, ContradictionKind, Memory } from "../api";

export interface ResolvedPair {
  /** The pair's own identity, kept whole so a verdict can be recorded against
   *  it even when neither memory could be resolved for display. */
  readonly id: { readonly a: string; readonly b: string };
  readonly kind: ContradictionKind;
  readonly a: Memory | null;
  readonly b: Memory | null;
  /** Whether both sides could be shown. **A pair is never dropped for being
   *  incomplete** — recall filters by confidence, so a missing side is
   *  usually a real memory that is simply not on this page, and hiding the
   *  disagreement would suppress exactly what this surface exists to say. */
  readonly complete: boolean;
}

export function pairsFor(
  contradictions: readonly Contradiction[],
  memories: readonly Memory[],
): readonly ResolvedPair[] {
  const byId = new Map(memories.map((memory) => [memory.id, memory]));
  return contradictions.map((pair) => {
    const a = byId.get(pair.a) ?? null;
    const b = byId.get(pair.b) ?? null;
    return {
      id: { a: pair.a, b: pair.b },
      kind: pair.kind,
      a,
      b,
      complete: a !== null && b !== null,
    };
  });
}

/** What to say about why a pair is here.
 *
 *  **"A person said so" and "a rule guessed" must not read alike**: one is a
 *  fact to act on, the other a candidate to judge, and a reviewer's next move
 *  is different for each. */
export function kindLabel(kind: ContradictionKind): string {
  switch (kind) {
    case "declared":
      return "Someone recorded that these disagree";
    case "confirmed":
      // Deliberately not "resolved": the engine keeps a confirmed pair in the
      // queue, because confirming a contradiction is not settling it.
      return "A reviewer agreed these disagree";
    case "candidate":
      return "These might disagree — two current decisions about the same thing";
  }
}
