import { describe, expect, it } from "vitest";
import {
  LAYER_WIDTH,
  ROW_HEIGHT,
  type LineageGraph,
  layers,
  positions,
} from "./lineage";

function node(id: string, name = id) {
  return { id, name, kind: "table", deleted: false };
}
function edge(from: string, to: string) {
  return {
    id: `${from}->${to}`,
    fromAssetId: from,
    toAssetId: to,
    relationship: "feeds",
    source: "manual",
  };
}

/** a → b → c, with the walk rooted at b. */
function chain(): LineageGraph {
  return {
    rootId: "b",
    nodes: [node("a"), node("b"), node("c")],
    edges: [edge("a", "b"), edge("b", "c")],
  };
}

describe("which layer a node belongs in", () => {
  it("puts the root at zero", () => {
    expect(layers(chain()).get("b")).toBe(0);
  });

  /** The sign is the whole point. A layout that placed upstream and downstream
   *  by raw distance would put a source and a consumer in the same column and
   *  quietly invert which way the data flows. */
  it("puts what feeds the root behind it and what it feeds ahead", () => {
    const depth = layers(chain());

    expect(depth.get("a")).toBe(-1);
    expect(depth.get("c")).toBe(1);
  });

  it("counts distance, not just direction", () => {
    const depth = layers({
      rootId: "a",
      nodes: [node("a"), node("b"), node("c")],
      edges: [edge("a", "b"), edge("b", "c")],
    });

    expect(depth.get("b")).toBe(1);
    expect(depth.get("c")).toBe(2);
  });

  /** A diamond's shared node takes the *shorter* path's layer, because
   *  breadth-first reaches it there first — and a node drawn two layers out
   *  when one of its inputs is adjacent reads as further from the root than it
   *  is. */
  it("places a diamond's shared node once", () => {
    const depth = layers({
      rootId: "a",
      nodes: [node("a"), node("b"), node("c"), node("d")],
      edges: [edge("a", "b"), edge("a", "c"), edge("b", "d"), edge("c", "d")],
    });

    expect(depth.get("b")).toBe(1);
    expect(depth.get("c")).toBe(1);
    expect(depth.get("d")).toBe(2);
  });

  /** A cycle must terminate. The graph is called acyclic because it should be,
   *  not because anything stops somebody asserting otherwise. */
  it("terminates on a cycle", () => {
    const depth = layers({
      rootId: "a",
      nodes: [node("a"), node("b"), node("c")],
      edges: [edge("a", "b"), edge("b", "c"), edge("c", "a")],
    });

    expect(depth.size).toBe(3);
    expect(depth.get("a")).toBe(0);
  });
});

describe("placing the nodes", () => {
  it("separates layers horizontally, in flow order", () => {
    const placed = positions(chain());
    const at = (id: string) => placed.find((p) => p.id === id)!;

    expect(at("a").x).toBe(-LAYER_WIDTH);
    expect(at("b").x).toBe(0);
    expect(at("c").x).toBe(LAYER_WIDTH);
  });

  /** Fetch order varies between runs. A lineage graph that reshuffles on reload
   *  is one nobody can describe to a colleague over a call. */
  it("orders a layer by name, not by arrival", () => {
    const graph: LineageGraph = {
      rootId: "root",
      nodes: [node("root"), node("z", "zebra"), node("a", "aardvark")],
      edges: [edge("root", "z"), edge("root", "a")],
    };

    const placed = positions(graph);
    const zebra = placed.find((p) => p.id === "z")!;
    const aardvark = placed.find((p) => p.id === "a")!;

    expect(aardvark.y).toBeLessThan(zebra.y);
    // And the same input in the other order lands identically.
    const reversed = positions({ ...graph, nodes: [...graph.nodes].reverse() });
    expect(reversed.find((p) => p.id === "a")!.y).toBe(aardvark.y);
  });

  /** Centred, so a wide layer does not push every other layer's single node to
   *  the top of the canvas. */
  it("centres a layer around zero", () => {
    const placed = positions({
      rootId: "root",
      nodes: [node("root"), node("a"), node("b")],
      edges: [edge("root", "a"), edge("root", "b")],
    });

    const ys = placed.filter((p) => p.x > 0).map((p) => p.y);
    expect(ys.reduce((a, b) => a + b, 0)).toBe(0);
  });

  /** Spacing asserted as a distance, not as an ordering. Ordering and the
   *  centring sum both survive a layout that packs a whole layer into a few
   *  pixels — every node overlapping, and the picture unreadable while every
   *  other assertion here still passes. */
  it("separates siblings by a full row", () => {
    const placed = positions({
      rootId: "root",
      nodes: [node("root"), node("a"), node("b"), node("c")],
      edges: [edge("root", "a"), edge("root", "b"), edge("root", "c")],
    });

    const ys = placed
      .filter((p) => p.x > 0)
      .map((p) => p.y)
      .sort((l, r) => l - r);

    expect(ys).toEqual([-ROW_HEIGHT, 0, ROW_HEIGHT]);
  });

  it("places every node exactly once", () => {
    const placed = positions(chain());

    expect(placed).toHaveLength(3);
    expect(new Set(placed.map((p) => p.id)).size).toBe(3);
  });

  /** A node the walk never reached still came back from the server, so
   *  something connects it. Dropping it would show a smaller graph than
   *  exists — the failure mode a lineage picture must not have. */
  it("still places a node the walk did not reach", () => {
    const placed = positions({
      rootId: "a",
      nodes: [node("a"), node("orphan")],
      edges: [],
    });

    expect(placed).toHaveLength(2);
  });
});
