/** Everything decidable about one asset's structural analytics — Plan 112
 *  Slice C.
 *
 *  **`GET /assets/{id}/analytics` and `api.assetAnalytics` both existed and
 *  `grep assetAnalytics ui/src` matched only the definition.** The same defect
 *  Plan 111 named twice: capability that stops at the API layer is reachable
 *  by an integrator and by nobody else.
 *
 *  **Bounded, and the panel must say so.** The server computes over the same
 *  already-authorized, already-capped walk the explorer draws, never over the
 *  whole graph — Epic 38's purity boundary. So "3 nodes" from a truncated walk
 *  is a claim the server never made, and `describeAnalytics` refuses to make
 *  it.
 *
 *  Nothing here names an edge type. The server derives them from the data it
 *  walked, which is what lets a hospitality deployment read its own
 *  vocabulary out of the same panel. */

import type { AssetAnalytics } from "../api";
import { kindsFromAnalytics } from "./edgeFilter";

export interface ConnectivityRow {
  readonly id: string;
  /** The `Sid`'s local part — `1:abc` reads as `abc`. The namespace code is
   *  identity, not information, once every row shares it. */
  readonly label: string;
  readonly inDegree: number;
  readonly outDegree: number;
  /** Connected to nothing else *in this neighbourhood* — which is not the same
   *  as connected to nothing, and the panel's wording has to keep that
   *  distinction. */
  readonly orphan: boolean;
}

/** One row per node, most connected first.
 *
 *  **The three vectors are index-aligned and joining them by position is the
 *  server's stated contract.** A row assembled by reordering either side would
 *  attribute one node's connectivity to another — a wrong answer that looks
 *  like a working table, which is why a length mismatch throws rather than
 *  rendering the shorter prefix. */
export function connectivityRows(
  analytics: AssetAnalytics,
  /** Names the caller already resolved for these nodes — the canvas above has
   *  them. **Prefer a known name, fall back to the identifier, never invent
   *  one**, exactly as `paths.ts` does: an asset's graph identity is a UUID,
   *  and a table of hex strings makes the reader match them by eye. */
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
 *  **Truncation is stated, never inferred from a count.** The reader's
 *  conclusion from a bare "3 nodes" is that the neighbourhood has three nodes;
 *  if the walk stopped early that is a stronger claim than the server made. */
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
