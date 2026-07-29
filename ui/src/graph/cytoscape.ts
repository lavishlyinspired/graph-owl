/** The model, as Cytoscape wants it.
 *
 *  `00f-ui-architecture.md` requires graph tests to assert **the model, not the
 *  picture**, so the part of the renderer swap that can be wrong in a way a
 *  reader would act on lives here as a pure function: which nodes exist, which
 *  classes they carry, and whether the layout is deterministic. What Cytoscape
 *  paints from that is a drawing decision.
 *
 *  This is the payoff of `GraphView` having been renderer-agnostic from the
 *  start — the model and its 116 mutation-tested assertions are untouched by
 *  the swap from SVG.
 */

import type { GraphEdge, GraphNode } from "../api";
import type { Change, DiffEdge, DiffNode } from "./diff";

/** A Cytoscape element. Typed structurally rather than imported, so this module
 *  and its tests do not need the library — the shape *is* the contract. */
export interface Element {
  readonly group: "nodes" | "edges";
  readonly data: {
    readonly id: string;
    readonly label?: string;
    readonly source?: string;
    readonly target?: string;
  };
  readonly classes: string;
}

/** What the canvas is being asked to draw. */
export interface Picture {
  readonly seedId: string;
  readonly nodes: readonly (GraphNode | DiffNode)[];
  readonly edges: readonly (GraphEdge | DiffEdge)[];
  /** Nodes already walked; the rest are drawn as expandable. */
  readonly expanded: readonly string[];
  /** Nodes hiding neighbours the budget cut. */
  readonly truncatedAt: readonly string[];
}

function changeOf(item: { change?: Change }): Change {
  return item.change ?? "unchanged";
}

/** A stable id for an edge.
 *
 *  Both endpoints **and** the relationship, matching `model.ts`: `a contains b`
 *  and `a feeds b` are two facts about one pair, and an id built from the
 *  endpoints alone would make Cytoscape silently drop the second — an edge
 *  vanishing from the picture because of how it was keyed, not because of
 *  anything true about the graph.
 */
export function edgeId(edge: GraphEdge): string {
  return `${edge.from}→${edge.to}→${edge.relationship}`;
}

/** Classes drive styling, and each one is a fact the reader can act on.
 *
 *  `removed` is here because `op = false` is a retraction: a node that existed
 *  at the earlier instant and not the later one is **still drawn**, marked, so
 *  the picture shows a deletion rather than an absence. `truncated` marks the
 *  node that is hiding something rather than the canvas as a whole, so a reader
 *  can tell *where* the picture is incomplete.
 */
export function nodeClasses(
  node: GraphNode | DiffNode,
  picture: Picture,
): string {
  const classes: string[] = [changeOf(node as DiffNode)];
  if (node.id === picture.seedId) classes.push("seed");
  if (!picture.expanded.includes(node.id)) classes.push("expandable");
  if (picture.truncatedAt.includes(node.id)) classes.push("truncated");
  // A node the reader may not see keeps its place in the picture but not its
  // label — removing it would claim a smaller neighbourhood than exists.
  if (node.kind === null) classes.push("hidden-kind");
  return classes.join(" ");
}

/** The whole picture as Cytoscape elements, nodes before edges.
 *
 *  Order matters: Cytoscape rejects an edge whose endpoints it has not seen, so
 *  an edge listed first is dropped rather than deferred.
 */
export function toElements(picture: Picture): Element[] {
  const present = new Set(picture.nodes.map((node) => node.id));

  const nodes: Element[] = picture.nodes.map((node) => ({
    group: "nodes" as const,
    data: { id: node.id, label: node.name },
    classes: nodeClasses(node, picture),
  }));

  const edges: Element[] = picture.edges
    // An edge to a node that is not in the picture is dropped rather than
    // drawn to nowhere. It happens legitimately: a diff can hold an edge whose
    // far end was filtered out by authorization, and Cytoscape would throw on
    // it rather than skip it.
    .filter((edge) => present.has(edge.from) && present.has(edge.to))
    .map((edge) => ({
      group: "edges" as const,
      data: {
        id: edgeId(edge),
        source: edge.from,
        target: edge.to,
        label: edge.relationship,
      },
      classes: changeOf(edge as DiffEdge),
    }));

  return [...nodes, ...edges];
}

/** The layout.
 *
 *  **`breadthfirst`, rooted at the seed, and deterministic.** `00f` requires a
 *  layout that settles the same way every run: a force simulation lands
 *  somewhere slightly different each time, so the same neighbourhood never
 *  looks the same twice and nobody can say "the node on the left". Rings by
 *  distance from the seed also encode the one thing this picture is for, which
 *  a physics simulation encodes only incidentally.
 *
 *  `animate: false` for the same reason it is deterministic — and because an
 *  animation on every expand moves the whole picture under a reader who was
 *  looking at one part of it.
 */
export function layoutOptions(seedId: string) {
  return {
    name: "breadthfirst",
    roots: [seedId],
    directed: false,
    animate: false,
    // Deterministic tie-breaking. Without it Cytoscape orders siblings by
    // insertion, which is fetch order, which is not stable across runs.
    maximal: true,
    grid: true,
    spacingFactor: 1.1,
    padding: 24,
  };
}

/** Whether the canvas should use WebGL.
 *
 *  On above a threshold, off below it. Not because Canvas is inadequate at
 *  small sizes — it is fine, and *better*: WebGL has a fixed context-creation
 *  cost and a texture atlas that only pays for itself once there is enough to
 *  draw, so switching it on for a six-node neighbourhood makes the common case
 *  slower to open.
 *
 *  256 because that is roughly where an SVG/Canvas renderer's per-element cost
 *  starts to show on a mid-range laptop, and it is an order of magnitude below
 *  the 10,000-node interactivity budget in `00f` — so the budget is never the
 *  thing being tested here. `00f` rejects a *hybrid* that swaps renderers
 *  mid-session; this is chosen once, when the canvas is created.
 */
export const WEBGL_THRESHOLD = 256;

export function wantsWebgl(nodeCount: number): boolean {
  return nodeCount >= WEBGL_THRESHOLD;
}
