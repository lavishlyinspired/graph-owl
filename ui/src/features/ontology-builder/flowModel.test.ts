import { describe, expect, it } from "vitest";
import type { OntologyModel } from "./types";
import { toFlowElements } from "./flowModel";

function sampleModel(): OntologyModel {
  return {
    entityTypes: [
      {
        id: "e1",
        name: "Customer",
        displayName: "Customer",
        description: "",
        color: "#2a78d6",
        attributes: [],
      },
      {
        id: "e2",
        name: "Order",
        displayName: "Order",
        description: "",
        color: "#eb6834",
        attributes: [],
      },
    ],
    relationships: [
      {
        id: "r1",
        fromEntityTypeId: "e1",
        toEntityTypeId: "e2",
        name: "places",
        displayName: "places",
        description: "",
        cardinality: "oneToMany",
      },
    ],
    interactions: [],
    referenceData: [],
    sources: [],
  };
}

describe("toFlowElements", () => {
  it("emits one node per entity type and one edge per relationship", () => {
    const { nodes, edges } = toFlowElements(sampleModel());
    expect(nodes).toHaveLength(2);
    expect(edges).toHaveLength(1);
  });

  it("carries the entity colour on the node", () => {
    const { nodes } = toFlowElements(sampleModel());
    const customer = nodes.find((n) => n.id === "e1");
    expect(customer?.color).toBe("#2a78d6");
  });

  it("carries a category icon derived from the entity's own colour and name", () => {
    const { nodes } = toFlowElements(sampleModel());
    const customer = nodes.find((n) => n.id === "e1");
    expect(customer?.icon).toMatch(/^data:image\/svg\+xml,/);
    expect(decodeURIComponent(customer!.icon)).toContain("#2a78d6");
  });

  it("gives different-coloured entities a different icon", () => {
    const { nodes } = toFlowElements(sampleModel());
    const customer = nodes.find((n) => n.id === "e1");
    const order = nodes.find((n) => n.id === "e2");
    expect(customer?.icon).not.toBe(order?.icon);
  });

  it("marks a self-loop relationship (same source and target) as such", () => {
    const model = sampleModel();
    const selfLoop = {
      ...model.relationships[0]!,
      id: "r2",
      fromEntityTypeId: "e1",
      toEntityTypeId: "e1",
    };
    const { edges } = toFlowElements({ ...model, relationships: [selfLoop] });
    const edge = edges.find((e) => e.id === "r2");
    expect(edge?.selfLoop).toBe(true);
  });

  it("does not mark an ordinary relationship as a self-loop", () => {
    const { edges } = toFlowElements(sampleModel());
    expect(edges[0]?.selfLoop).toBe(false);
  });
});
