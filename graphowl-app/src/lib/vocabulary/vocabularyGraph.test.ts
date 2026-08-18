import { describe, expect, it } from "vitest";
import { layoutTermGraph } from "./vocabularyGraph";
import type { SkosRelation } from "../api";

describe("layoutTermGraph — Plan 122a A7", () => {
  const terms = [
    { id: "a", name: "Customer" },
    { id: "b", name: "Individual Customer" },
    { id: "c", name: "Corporate Customer" },
  ];

  it("places every term exactly once, none dropped and none duplicated", () => {
    const { nodes } = layoutTermGraph(terms, new Map());
    expect(nodes.map((n) => n.id).sort()).toEqual(["a", "b", "c"]);
  });

  it("spreads nodes around the circle, not stacked on the same point", () => {
    const { nodes } = layoutTermGraph(terms, new Map());
    const positions = new Set(nodes.map((n) => `${n.x.toFixed(1)},${n.y.toFixed(1)}`));
    expect(positions.size).toBe(3);
  });

  it("is a real circle: every node is equidistant from the given center", () => {
    const center = { x: 300, y: 300 };
    const radius = 150;
    const { nodes } = layoutTermGraph(terms, new Map(), radius, center);
    for (const node of nodes) {
      const distance = Math.hypot(node.x - center.x, node.y - center.y);
      expect(distance).toBeCloseTo(radius, 5);
    }
  });

  it("turns a broader relation into an edge between the two real terms", () => {
    const relations = new Map<string, readonly SkosRelation[]>([
      ["b", [{ kind: "broader", target: "a" }]],
    ]);
    const { edges } = layoutTermGraph(terms, relations);
    expect(edges).toEqual([{ from: "b", to: "a", kind: "broader" }]);
  });

  it("drops an edge whose target is not one of the given terms, rather than rendering a phantom node", () => {
    const relations = new Map<string, readonly SkosRelation[]>([
      ["b", [{ kind: "related", target: "not-in-this-glossary" }]],
    ]);
    const { edges } = layoutTermGraph(terms, relations);
    expect(edges).toEqual([]);
  });

  it("includes every relation kind a term declares, not just the hierarchy ones", () => {
    const relations = new Map<string, readonly SkosRelation[]>([
      ["b", [{ kind: "broader", target: "a" }, { kind: "related", target: "c" }]],
    ]);
    const { edges } = layoutTermGraph(terms, relations);
    expect(edges).toHaveLength(2);
    expect(edges.map((e) => e.kind).sort()).toEqual(["broader", "related"]);
  });

  it("is empty, not broken, for a glossary with no terms", () => {
    expect(layoutTermGraph([], new Map())).toEqual({ nodes: [], edges: [] });
  });

  it("places a single term at the circle's edge, not at its center", () => {
    const center = { x: 100, y: 100 };
    const { nodes } = layoutTermGraph([{ id: "only", name: "Only" }], new Map(), 50, center);
    expect(nodes[0]).toMatchObject({ x: 150, y: 100 });
  });
});
