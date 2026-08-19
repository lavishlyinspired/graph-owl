import { describe, expect, it } from "vitest";
import { layout } from "./graphLayout";
import type { GraphEdge, GraphNode } from "./api";

const node = (id: string, isSeed = false): GraphNode => ({
  id,
  label: id,
  badge: "X",
  type_line: null,
  is_seed: isSeed,
});

describe("layout", () => {
  it("places every node exactly once", () => {
    const nodes = [node("a", true), node("b"), node("c")];
    const edges: GraphEdge[] = [];

    const positions = layout(nodes, edges, 400, 300);

    expect(positions.size).toBe(3);
    for (const id of ["a", "b", "c"]) expect(positions.has(id)).toBe(true);
  });

  it("is deterministic — the same input always lands in the same place", () => {
    // A layout that jitters on every render is unreadable: a reviewer who
    // looks away and back should not have to re-find every node.
    const nodes = [node("a", true), node("b"), node("c"), node("d")];
    const edges: GraphEdge[] = [
      { from: "a", to: "b", label: "x", style: "solid", highlighted: true },
      { from: "b", to: "c", label: "y", style: "solid", highlighted: false },
    ];

    const first = layout(nodes, edges, 500, 400);
    const second = layout(nodes, edges, 500, 400);

    for (const id of ["a", "b", "c", "d"]) {
      expect(first.get(id)).toEqual(second.get(id));
    }
  });

  it("keeps connected nodes closer together than an unrelated pair", () => {
    const nodes = [node("a", true), node("b"), node("c")];
    const edges: GraphEdge[] = [
      { from: "a", to: "b", label: "x", style: "solid", highlighted: true },
    ];

    const positions = layout(nodes, edges, 600, 500);
    const dist = (p: string, q: string) => {
      const a = positions.get(p)!;
      const b = positions.get(q)!;
      return Math.hypot(a.x - b.x, a.y - b.y);
    };

    expect(dist("a", "b")).toBeLessThan(dist("a", "c"));
  });

  it("keeps every position inside the given bounds", () => {
    const nodes = Array.from({ length: 8 }, (_, i) => node(`n${i}`, i === 0));
    const edges: GraphEdge[] = [];

    const positions = layout(nodes, edges, 400, 300);

    for (const p of positions.values()) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(400);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(300);
    }
  });

  it("handles a single node without dividing by zero", () => {
    const positions = layout([node("a", true)], [], 400, 300);

    expect(positions.get("a")).toBeDefined();
    expect(Number.isFinite(positions.get("a")!.x)).toBe(true);
  });

  it("handles no nodes at all", () => {
    expect(layout([], [], 400, 300).size).toBe(0);
  });
});
