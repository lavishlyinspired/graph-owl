/** Reshaping a `GraphView` into the two tables the mockup's Entity Overview
 *  shows: FACTS (what this subject asserts or has concluded) and IMPACT IF
 *  CHANGED (what else in the graph points at it). Both were previously
 *  computed inline, once, only for a catalog asset — pulled out here so
 *  `entity.tsx`'s graph-only path (a GST invoice, say — most of this
 *  console's real data) can build the identical tables from
 *  `fetchGraphContext` instead of `fetchAssetGraph`. */

import type { GraphEdge, GraphView } from "./api";

export interface EntityFact {
  readonly relationship: string;
  readonly target: string;
  readonly derived: boolean;
}

/** One row per outgoing edge, naming the target by its resolved label —
 *  falling back to the bare id only when the target has no node in this
 *  picture to resolve it from. */
export function factsFromEdges(view: GraphView): readonly EntityFact[] {
  const byId = new Map(view.nodes.map((node) => [node.id, node.name]));
  return view.edges.map((edge) => ({
    relationship: edge.relationship,
    target: byId.get(edge.to) ?? edge.to,
    // The reasoner concluded this edge; nobody asserted it. Absent reads as
    // asserted — understating rather than overstating what was inferred,
    // the same convention `GraphEdge.derived` itself documents.
    derived: edge.derived === true,
  }));
}

export interface EntityImpactRow {
  readonly label: string;
  readonly n: number;
}

/** How many incoming edges carry each relationship — "if this subject
 *  changed, this many facts elsewhere would need re-checking," grouped by
 *  what kind of fact they are rather than left as a bare count. */
export function impactFromEdges(edges: readonly GraphEdge[]): readonly EntityImpactRow[] {
  const grouped = new Map<string, number>();
  for (const edge of edges) {
    grouped.set(edge.relationship, (grouped.get(edge.relationship) ?? 0) + 1);
  }
  return [...grouped.entries()].map(([label, n]) => ({ label, n }));
}
