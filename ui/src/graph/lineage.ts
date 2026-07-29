/** Lineage as a layered DAG.
 *
 *  `00f-ui-architecture.md` keeps two renderers on purpose: exploration is an
 *  arbitrary cyclic graph at scale, where WebGL is the point and a layered
 *  layout is meaningless; lineage is a **directed acyclic graph read
 *  left-to-right**, where the layered layout *is* the point and node count is
 *  modest. One library doing both does neither well.
 *
 *  The positioning lives here rather than in the component, for the same reason
 *  the Cytoscape mapping does: which node sits in which layer is a fact a reader
 *  acts on — "these three all feed that one" — and it has to be assertable
 *  without rendering anything.
 */

export interface LineageNode {
  readonly id: string;
  readonly name: string;
  readonly kind: string | null;
  /** A tombstoned asset stays in the picture. "Nothing downstream" and "the
   *  downstream was deleted" are opposite conclusions. */
  readonly deleted: boolean;
}

export interface LineageEdge {
  readonly id: string;
  readonly fromAssetId: string;
  readonly toAssetId: string;
  readonly relationship: string;
  readonly source: string;
  readonly query?: string | null;
  readonly description?: string | null;
}

export interface LineageGraph {
  readonly rootId: string;
  readonly nodes: readonly LineageNode[];
  readonly edges: readonly LineageEdge[];
}

/** A node's depth from the root, negative upstream and positive downstream.
 *
 *  **The sign is the whole point.** A lineage picture read left-to-right only
 *  means anything if "what feeds this" is consistently on one side; a layout
 *  that placed upstream and downstream by raw distance would put a source and a
 *  consumer in the same column and quietly invert the reader's understanding of
 *  which way the data flows.
 */
export function layers(graph: LineageGraph): Map<string, number> {
  const depth = new Map<string, number>([[graph.rootId, 0]]);

  const step = (forward: boolean) => {
    let frontier = [graph.rootId];
    let distance = 0;
    while (frontier.length > 0) {
      distance += 1;
      const next: string[] = [];
      for (const edge of graph.edges) {
        const near = forward ? edge.fromAssetId : edge.toAssetId;
        const far = forward ? edge.toAssetId : edge.fromAssetId;
        if (!frontier.includes(near) || depth.has(far)) continue;
        depth.set(far, forward ? distance : -distance);
        next.push(far);
      }
      frontier = next;
    }
  };

  step(true);
  step(false);
  return depth;
}

export interface Positioned {
  readonly id: string;
  readonly x: number;
  readonly y: number;
}

/** Horizontal spacing between layers, and vertical between siblings.
 *
 *  Wide enough that an edge between adjacent layers is visibly horizontal
 *  rather than a short diagonal — the direction of flow is the one thing this
 *  picture must not leave ambiguous.
 */
export const LAYER_WIDTH = 240;
export const ROW_HEIGHT = 72;

/** Place every node, deterministically.
 *
 *  Within a layer, nodes are ordered by **name**, not by arrival. Fetch order
 *  varies between runs, and a lineage graph that reshuffles when you reload is
 *  one nobody can describe to a colleague over a call.
 */
export function positions(graph: LineageGraph): Positioned[] {
  const depth = layers(graph);
  const byLayer = new Map<number, LineageNode[]>();

  for (const node of graph.nodes) {
    // A node the walk never reached sits in the root's layer rather than being
    // dropped: it came back from the server, so something connects it, and
    // hiding it would show a smaller graph than exists.
    const layer = depth.get(node.id) ?? 0;
    const row = byLayer.get(layer) ?? [];
    row.push(node);
    byLayer.set(layer, row);
  }

  const placed: Positioned[] = [];
  for (const [layer, row] of byLayer) {
    const ordered = [...row].sort((a, b) => a.name.localeCompare(b.name));
    ordered.forEach((node, index) => {
      placed.push({
        id: node.id,
        x: layer * LAYER_WIDTH,
        // Centred on the layer, so a wide layer does not push every other
        // layer's single node to the top of the canvas.
        y: (index - (ordered.length - 1) / 2) * ROW_HEIGHT,
      });
    });
  }
  return placed;
}
