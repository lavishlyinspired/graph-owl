/** Everything decidable about one neighbourhood's structural analytics —
 *  `GET /assets/{id}/analytics` / `POST /graph/context/analytics`
 *  (`graph-owl-analytics`, Epic 38's `petgraph`-backed degree centrality,
 *  connected components and orphan detection).
 *
 *  **The capability existed and only the agent could reach it.** Both
 *  routes were added specifically so the console could ask "how connected
 *  is this", and until now nothing in `graphowl-app` called either.
 *
 *  **Bounded, and the panel must say so.** The server computes over the
 *  same already-authorized, already-capped walk the explorer draws, never
 *  the whole graph — Epic 38's purity boundary forbids that on a
 *  synchronous request. So "3 nodes" from a truncated walk is a claim the
 *  server never made, and `describeAnalytics` refuses to make it either.
 *
 *  Nothing here names an edge type — the vocabulary comes from the walk
 *  itself, so a hospitality deployment reads its own predicates out of the
 *  same panel a GST one uses. */

import type { AssetAnalytics } from "../api";
import { kindsFromAnalytics } from "./edgeFilter";

export interface ConnectivityRow {
  readonly id: string;
  /** The `Sid`'s local part when no caller-supplied name exists —
   *  `1:abc` reads as `abc`. */
  readonly label: string;
  readonly inDegree: number;
  readonly outDegree: number;
  /** Connected to nothing else *in this neighbourhood* — not the same
   *  claim as connected to nothing. */
  readonly orphan: boolean;
}

/** One row per node, most connected first.
 *
 *  **The three vectors are index-aligned, and that is the server's stated
 *  contract** — a row assembled by reordering either side would attribute
 *  one node's connectivity to another, so a length mismatch throws rather
 *  than silently rendering a shorter, wrong prefix. */
export function connectivityRows(
  analytics: AssetAnalytics,
  /** Names the caller already resolved — prefer a known name, fall back
   *  to the identifier, never invent one. */
  names: ReadonlyMap<string, string> = new Map(),
): readonly ConnectivityRow[] {
  const { nodes, inDegree, outDegree, orphans } = analytics;
  if (nodes.length !== inDegree.length || nodes.length !== outDegree.length) {
    throw new Error(
      `analytics vectors disagree: ${nodes.length} nodes, ${inDegree.length} in-degrees, ${outDegree.length} out-degrees`,
    );
  }
  const isOrphan = new Set(orphans);
  return nodes
    .map((id, index) => ({
      id,
      label: names.get(id) ?? id.slice(id.indexOf(":") + 1),
      inDegree: inDegree[index]!,
      outDegree: outDegree[index]!,
      orphan: isOrphan.has(id),
    }))
    .sort((a, b) => b.inDegree + b.outDegree - (a.inDegree + a.outDegree));
}

/** One sentence saying what these numbers cover.
 *
 *  **Truncation is stated, never inferred from a count.** A reader's
 *  conclusion from a bare "3 nodes" is that the neighbourhood has three
 *  nodes; if the walk stopped early that is a stronger claim than the
 *  server made. */
export function describeAnalytics(analytics: AssetAnalytics): string {
  const scope = analytics.truncated
    ? `${analytics.nodes.length} nodes reached before the walk stopped at its limit`
    : `${analytics.nodes.length} nodes in this neighbourhood`;
  const edges =
    analytics.edgeTypes.length === 0
      ? "no relationships between them"
      : `connected by ${kindsFromAnalytics(analytics.edgeTypes).join(", ")}`;
  return `${scope}, ${edges}.`;
}
