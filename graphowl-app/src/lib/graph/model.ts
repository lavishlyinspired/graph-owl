/** The graph as the console holds it, independent of what draws it.
 *
 *  `00f-ui-architecture.md`: graph tests assert the model, not the picture.
 *  Everything that could be wrong in a way a user would act on — a node
 *  counted twice, an expansion that hid what it could not fit — is decided
 *  here, where it can be tested without rendering anything.
 *
 *  Epic 40 decision 2: the canvas opens on a seed and grows by explicit
 *  expansion. There is no "show everything" — on a real estate it is a hang,
 *  and on a demo estate it is a hairball that teaches nothing.
 *
 *  Ported from `ui/src/graph/model.ts` (Plan 122a A3) — pure logic, no
 *  renderer dependency, unchanged from the original. */

import type { GraphEdge, GraphNode, GraphView } from "../api";

export interface GraphModel {
  /** The entity the canvas opened on. Always present, always at the centre. */
  readonly seedId: string;
  readonly nodes: readonly GraphNode[];
  readonly edges: readonly GraphEdge[];
  /** Nodes already walked. Expanding one again is a no-op, so the control for
   *  it can be hidden rather than left to produce nothing. */
  readonly expanded: readonly string[];
  /** Any part of this picture is incomplete. Sticky by construction: see
   *  {@link expand}. */
  readonly truncated: boolean;
  /** Which nodes have more neighbours than were fetched, so the marker lands
   *  on the node that is actually hiding something rather than on the canvas
   *  as a whole. */
  readonly truncatedAt: readonly string[];
}

/** Identity is the node's own id, never its position in a response.
 *
 *  The same entity reached by two different walks has to be one node or
 *  selection, deep links, degree counts and diff all break at once. */
function mergeNodes(existing: readonly GraphNode[], incoming: readonly GraphNode[]): GraphNode[] {
  const seen = new Set(existing.map((node) => node.id));
  return [...existing, ...incoming.filter((node) => !seen.has(node.id))];
}

/** An edge is a *fact*, and both endpoints plus the relationship name it.
 *
 *  `a contains b` and `a feeds b` are two different things to know about the
 *  same pair; collapsing them to one edge would hide one of them. */
function edgeKey(edge: GraphEdge): string {
  return `${edge.from} ${edge.to} ${edge.relationship}`;
}

function mergeEdges(existing: readonly GraphEdge[], incoming: readonly GraphEdge[]): GraphEdge[] {
  const seen = new Set(existing.map(edgeKey));
  return [...existing, ...incoming.filter((edge) => !seen.has(edgeKey(edge)))];
}

function withMember(members: readonly string[], id: string): string[] {
  return members.includes(id) ? [...members] : [...members, id];
}

/** Open the canvas on one entity. */
export function seed(seedId: string, view: GraphView): GraphModel {
  return {
    seedId,
    nodes: [...view.nodes],
    edges: [...view.edges],
    expanded: [seedId],
    truncated: view.truncated,
    truncatedAt: view.truncated ? [seedId] : [],
  };
}

/** One expansion, and what the walk of that node returned.
 *
 *  `view: null` means **the node did not exist at that instant** — the only
 *  honest answer when replaying today's expansions against a past graph. */
export interface ReplayStep {
  readonly id: string;
  readonly view: GraphView | null;
}

/** Rebuild a model from a seed plus an ordered list of expansions.
 *
 *  This is what makes a *comparison* honest. The picture on screen is a seed
 *  walk plus everything the reader expanded; comparing it against a bare seed
 *  walk at the earlier instant reports every expanded node as **added**,
 *  regardless of when it actually arrived — the one failure that makes a diff
 *  screen worse than no diff screen, because it invents changes rather than
 *  missing them.
 *
 *  A step whose `view` is `null` is **skipped rather than treated as empty**,
 *  and the distinction is the whole point. A node that did not exist at the
 *  earlier instant contributes nothing to the earlier picture and is then
 *  correctly reported as added. Marking it expanded-but-empty would instead
 *  claim it existed with no neighbours, and every edge it really had would
 *  read as added too.
 *
 *  Skipping also must not set truncation: an absent node is not an omission
 *  this console made, and saying so would mark the comparison `partial` and
 *  suppress the very deletions it is there to show. */
export function replay(
  seedId: string,
  seedView: GraphView,
  steps: readonly ReplayStep[],
): GraphModel {
  return steps.reduce(
    (model, step) => (step.view === null ? model : expand(model, step.id, step.view)),
    seed(seedId, seedView),
  );
}

/** The expansions a reader performed, in order, excluding the seed.
 *
 *  The seed is excluded because it is not an expansion — it is where the
 *  canvas opened, and replaying it would walk the same node twice. */
export function performedExpansions(model: GraphModel): readonly string[] {
  return model.expanded.filter((id) => id !== model.seedId);
}

/** Grow the picture by one node's neighbourhood.
 *
 *  Truncation is **sticky**. A later complete expansion does not undo an
 *  earlier omission: the picture still has a hole in it, and a canvas that
 *  stopped saying so would let someone conclude "nothing else depends on
 *  this" from an absence this console created. */
export function expand(model: GraphModel, nodeId: string, view: GraphView): GraphModel {
  return {
    seedId: model.seedId,
    nodes: mergeNodes(model.nodes, view.nodes),
    edges: mergeEdges(model.edges, view.edges),
    expanded: withMember(model.expanded, nodeId),
    truncated: model.truncated || view.truncated,
    truncatedAt: view.truncated ? withMember(model.truncatedAt, nodeId) : [...model.truncatedAt],
  };
}
