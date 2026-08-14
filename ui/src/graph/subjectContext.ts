/** Everything decidable about rendering a subject's neighbourhood as a list
 *  — Plan 113 Slice C.
 *
 *  **The list-first half `00f-ui-architecture.md` already requires of every
 *  graph picture**, built for a subject that has no catalog asset row at
 *  all — `GraphExplorer`'s own `<details>` section draws this same
 *  distinction for catalog assets ("a picture is not an accessible interface
 *  on its own"), and a GST invoice deserves the identical guarantee.
 *
 *  A full interactive canvas for `GraphContext` is a natural next step and
 *  deliberately not attempted here: `GraphCanvas`'s `Picture` type expects a
 *  catalog-shaped `GraphNode` (with a `kind`), which a bare pack subject does
 *  not have, and stretching that type is a slice of its own rather than a
 *  side effect of this one. */

import type { GraphContext } from "../api";
import { nodeLabel } from "./paths";

export interface SubjectNodeRow {
  readonly id: string;
  readonly label: string;
  readonly sources: readonly string[];
}

/** One row per node, named through the same known-name-else-identifier rule
 *  `paths.ts`'s `nodeLabel` already commits to — reused rather than
 *  re-implemented, so the two surfaces cannot drift into disagreeing about
 *  what counts as "known". */
export function nodeRows(
  context: GraphContext,
  names: ReadonlyMap<string, string>,
): readonly SubjectNodeRow[] {
  return context.nodes.map((node) => ({
    id: node.id,
    label: nodeLabel(node.id, names),
    sources: node.sources,
  }));
}

export interface SubjectEdgeRow {
  readonly from: string;
  readonly relationship: string;
  readonly to: string;
  readonly derived: boolean;
}

/** One row per edge, as subject/relationship/object — the triples this
 *  neighbourhood actually is, named the way a reader who wants to see raw
 *  facts rather than a rendered picture expects to read them. */
export function edgeRows(
  context: GraphContext,
  names: ReadonlyMap<string, string>,
): readonly SubjectEdgeRow[] {
  return context.edges.map((edge) => ({
    from: nodeLabel(edge.from, names),
    relationship: edge.relationship,
    to: nodeLabel(edge.to, names),
    derived: edge.derived,
  }));
}

/** One sentence stating what was found, and whether the walk stopped early.
 *
 *  **Truncation is stated, never inferred from a count** — the same rule
 *  `analytics.ts`'s `describeAnalytics` already applies, for the same
 *  reason: a walk that stopped early and one that genuinely exhausted the
 *  neighbourhood must not read alike. */
export function summarize(context: GraphContext): string {
  // The seed itself is always one of `nodes`, so "no neighbourhood" means
  // nodes.length <= 1 — the seed and nothing else.
  if (context.nodes.length <= 1 && context.edges.length === 0) {
    return "Nothing connected to this subject at this depth.";
  }
  const nodeCount = `${context.nodes.length} node${context.nodes.length === 1 ? "" : "s"}`;
  const edgeCount = `${context.edges.length} relationship${context.edges.length === 1 ? "" : "s"}`;
  const stopped = context.truncated ? " The walk stopped at its limit." : "";
  return `${nodeCount}, ${edgeCount}.${stopped}`;
}
