/** Two instants, compared.
 *
 *  Epic 40 decision 5 and Slice D: this exists only because `op = false` is a
 *  retraction rather than a delete (`00b-architecture.md`). A store that
 *  overwrote could not answer "what did this look like last month", so it
 *  could not answer "what changed since" either — not without a parallel audit
 *  table that can drift from the thing it audits.
 *
 *  Renderer-agnostic, like the model: what a canvas does with `removed` is a
 *  drawing decision, but *which* nodes are removed is a correctness one. */

import type { AssetKind, GraphEdge, GraphNode } from "../api";
import type { GraphModel } from "./model";

export type Change = "added" | "removed" | "changed" | "unchanged";

export interface DiffNode {
  readonly id: string;
  /** The name at the *later* instant where the node still exists, and the
   *  name it had otherwise — a diff that showed only the old name would label
   *  the picture with something that no longer exists. */
  readonly name: string;
  readonly kind: AssetKind | null;
  readonly change: Change;
  /** Present only on a rename. */
  readonly wasName?: string;
  /** A diffed node is always a catalog asset (`00c`: time-travel/lineage is
   *  asset-only) and so never carries a pack subject's semantic type — kept
   *  here, always absent in practice, only so `Picture["nodes"]`
   *  (`GraphNode | DiffNode`) stays one uniform shape to read rather than
   *  needing a type guard at every access site. */
  readonly semanticType?: string | null;
}

export interface DiffEdge extends GraphEdge {
  readonly change: Change;
}

export interface GraphDiff {
  readonly nodes: readonly DiffNode[];
  readonly edges: readonly DiffEdge[];
  readonly summary: { added: number; removed: number; changed: number };
  /** Either side was truncated, so an absence may be an omission rather than
   *  a deletion. Presenting a partial comparison as complete would invent
   *  deletions that never happened. */
  readonly partial: boolean;
}

function edgeKey(edge: GraphEdge): string {
  return `${edge.from} ${edge.to} ${edge.relationship}`;
}

function byId(nodes: readonly GraphNode[]): Map<string, GraphNode> {
  return new Map(nodes.map((node) => [node.id, node]));
}

/** Compare the graph at two transaction times.
 *
 *  A node present at `before` and absent at `after` is **removed and still
 *  listed**. Dropping it would render the past and the present identically,
 *  which is this screen's differentiator failing silently.
 *
 *  Written as two passes over the two sides rather than one pass over the
 *  union of their ids: iterating entries means every branch has the node in
 *  hand, so there is no id whose node might be missing and no fallback for a
 *  state that cannot occur. */
export function diff(before: GraphModel, after: GraphModel): GraphDiff {
  const wasNode = byId(before.nodes);
  const isNode = byId(after.nodes);

  const nodes: DiffNode[] = [];
  for (const [id, was] of wasNode) {
    const is = isNode.get(id);
    if (is === undefined) {
      nodes.push({ id, name: was.name, kind: was.kind, change: "removed" });
      continue;
    }
    if (was.name === is.name && was.kind === is.kind) {
      nodes.push({ id, name: is.name, kind: is.kind, change: "unchanged" });
      continue;
    }
    // Surviving the interval is not the same as being unchanged: a rename is
    // something someone needs to see, and reporting it as unchanged because
    // the id matched is how a diff view stops being trusted.
    nodes.push({
      id,
      name: is.name,
      kind: is.kind,
      change: "changed",
      ...(was.name === is.name ? {} : { wasName: was.name }),
    });
  }
  for (const [id, is] of isNode) {
    if (!wasNode.has(id)) {
      nodes.push({ id, name: is.name, kind: is.kind, change: "added" });
    }
  }

  const wasEdge = new Map(before.edges.map((edge) => [edgeKey(edge), edge]));
  const isEdge = new Map(after.edges.map((edge) => [edgeKey(edge), edge]));

  // An edge is identified by both endpoints *and* its relationship, so
  // re-typing one is a removal plus an addition. Nothing about an edge can
  // change while its identity holds — there is no `changed` case here.
  const edges: DiffEdge[] = [];
  for (const [key, was] of wasEdge) {
    edges.push({ ...was, change: isEdge.has(key) ? "unchanged" : "removed" });
  }
  for (const [key, is] of isEdge) {
    if (!wasEdge.has(key)) edges.push({ ...is, change: "added" });
  }

  const moved = [...nodes, ...edges];
  return {
    nodes,
    edges,
    summary: {
      added: moved.filter((item) => item.change === "added").length,
      removed: moved.filter((item) => item.change === "removed").length,
      changed: moved.filter((item) => item.change === "changed").length,
    },
    partial: before.truncated || after.truncated,
  };
}
