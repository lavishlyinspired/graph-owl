import { describe, expect, it } from "vitest";
import { computeLayout } from "./layout";

function chain() {
  return {
    nodes: [{ id: "a" }, { id: "b" }, { id: "c" }],
    edges: [
      { source: "a", target: "b" },
      { source: "b", target: "c" },
    ],
  };
}

describe("computeLayout", () => {
  it("places every node given a position, for each of the three modes", () => {
    for (const mode of ["radial", "tree", "force"] as const) {
      const { nodes, edges } = chain();
      const positions = computeLayout(nodes, edges, mode);
      for (const node of nodes) {
        expect(positions[node.id]).toBeDefined();
        expect(Number.isFinite(positions[node.id]!.x)).toBe(true);
        expect(Number.isFinite(positions[node.id]!.y)).toBe(true);
      }
    }
  });

  it("tree layout: a node's children sit strictly downstream (deeper) of their parent", () => {
    const { nodes, edges } = chain();
    const positions = computeLayout(nodes, edges, "tree");
    // Breadthfirst-equivalent: root shallowest, each hop deeper.
    expect(positions.b!.y).toBeGreaterThan(positions.a!.y);
    expect(positions.c!.y).toBeGreaterThan(positions.b!.y);
  });

  it("radial layout: each hop from the root sits farther from the centre", () => {
    const { nodes, edges } = chain();
    const positions = computeLayout(nodes, edges, "radial");
    const centre = { x: 0, y: 0 };
    const dist = (p: { x: number; y: number }) => Math.hypot(p.x - centre.x, p.y - centre.y);
    expect(dist(positions.b!)).toBeGreaterThan(dist(positions.a!));
    expect(dist(positions.c!)).toBeGreaterThan(dist(positions.b!));
  });

  it("does not collapse every node onto the same point", () => {
    const { nodes, edges } = chain();
    for (const mode of ["radial", "tree", "force"] as const) {
      const positions = computeLayout(nodes, edges, mode);
      const points = nodes.map((n) => `${positions[n.id]!.x},${positions[n.id]!.y}`);
      expect(new Set(points).size).toBe(nodes.length);
    }
  });

  it("handles a single isolated node without throwing", () => {
    const positions = computeLayout([{ id: "solo" }], [], "force");
    expect(positions.solo).toBeDefined();
  });

  it("handles an empty graph", () => {
    expect(computeLayout([], [], "tree")).toEqual({});
  });
});
