import { describe, expect, it } from "vitest";
import type { OntologyModel } from "./types";
import { toCytoscapeElements } from "./cytoscapeModel";

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

describe("toCytoscapeElements", () => {
  it("emits one node per entity type and one edge per relationship", () => {
    const elements = toCytoscapeElements(sampleModel());
    const nodes = elements.filter((e) => e.group === "nodes");
    const edges = elements.filter((e) => e.group === "edges");
    expect(nodes).toHaveLength(2);
    expect(edges).toHaveLength(1);
  });

  it("carries the entity colour on the node", () => {
    const elements = toCytoscapeElements(sampleModel());
    const customer = elements.find((e) => e.data.id === "e1");
    expect(customer?.data.color).toBe("#2a78d6");
  });

  it("marks self-loop relationships with a distinct class", () => {
    const model = sampleModel();
    const selfLoop = {
      ...model.relationships[0]!,
      id: "r2",
      fromEntityTypeId: "e1",
      toEntityTypeId: "e1",
    };
    const elements = toCytoscapeElements({ ...model, relationships: [selfLoop] });
    const edge = elements.find((e) => e.data.id === "r2");
    expect(edge?.classes).toContain("self-loop");
  });
});
