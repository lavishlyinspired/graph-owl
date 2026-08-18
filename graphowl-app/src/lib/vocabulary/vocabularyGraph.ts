/** Pure layout for the Vocabulary Studio's Graph tab (Plan 122a A7 AC:
 *  "bubble layout, connect two concepts") — a lightweight radial layout
 *  computed here rather than pulling in a graph-rendering library (G6 is
 *  already the 422KB Explore-only budget exception; a second heavy
 *  dependency for a simple term-relation view is not justified). */

import type { SkosRelation } from "../api";

export interface GraphNode {
  readonly id: string;
  readonly name: string;
  readonly x: number;
  readonly y: number;
}

export interface GraphEdge {
  readonly from: string;
  readonly to: string;
  readonly kind: SkosRelation["kind"];
}

export interface TermGraph {
  readonly nodes: readonly GraphNode[];
  readonly edges: readonly GraphEdge[];
}

export function layoutTermGraph(
  terms: readonly { readonly id: string; readonly name: string }[],
  relationsByTerm: ReadonlyMap<string, readonly SkosRelation[]>,
  radius = 200,
  center: { readonly x: number; readonly y: number } = { x: 250, y: 250 },
): TermGraph {
  const byId = new Set(terms.map((t) => t.id));

  const nodes = terms.map((term, index) => {
    const angle = (index / Math.max(terms.length, 1)) * 2 * Math.PI;
    return {
      id: term.id,
      name: term.name,
      x: center.x + radius * Math.cos(angle),
      y: center.y + radius * Math.sin(angle),
    };
  });

  const edges: GraphEdge[] = [];
  for (const [from, relations] of relationsByTerm) {
    for (const relation of relations) {
      if (byId.has(relation.target)) {
        edges.push({ from, to: relation.target, kind: relation.kind });
      }
    }
  }

  return { nodes, edges };
}
