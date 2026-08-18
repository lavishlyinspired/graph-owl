import { describe, expect, it } from "vitest";
import type { OntologyModel } from "./types";
import { filterModelByNamespace, namespaceOf, namespacesIn, toFlowElements } from "./flowModel";

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
        namespace: null,
      },
      {
        id: "e2",
        name: "Order",
        displayName: "Order",
        description: "",
        color: "#eb6834",
        attributes: [],
        namespace: null,
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

/** Plan 120 Slice B — the namespace-filtered graph view, restored after
 *  commit `5cd2fd4` deleted `OntologyEditor.tsx` and merged its Check/Save
 *  logic into `OntologyBuilder.tsx`'s Code tab without carrying the filter
 *  over. Same algorithm the deleted `ontologyDocument.ts` used (last `#`,
 *  else last `/`, else the whole IRI) — restored, not reinvented. */
describe("namespaceOf", () => {
  it("cuts at the last # when the IRI has one", () => {
    expect(namespaceOf("https://graph-owl.dev/packs/gst#Supplier")).toBe(
      "https://graph-owl.dev/packs/gst#",
    );
  });

  it("cuts at the last / when there is no #", () => {
    expect(namespaceOf("https://example.org/ns/Widget")).toBe("https://example.org/ns/");
  });

  it("returns the whole string when there is neither", () => {
    expect(namespaceOf("Widget")).toBe("Widget");
  });
});

describe("namespacesIn", () => {
  it("returns the distinct, sorted namespaces of every entity that has one", () => {
    const model = sampleModel();
    const mixed: OntologyModel = {
      ...model,
      entityTypes: [
        { ...model.entityTypes[0]!, namespace: "https://graph-owl.dev/packs/gst#" },
        { ...model.entityTypes[1]!, namespace: "https://graph-owl.dev/packs/hospitality#" },
      ],
    };
    expect(namespacesIn(mixed)).toEqual([
      "https://graph-owl.dev/packs/gst#",
      "https://graph-owl.dev/packs/hospitality#",
    ]);
  });

  it("omits entities with no namespace — a manually added entity is not a namespace of one", () => {
    const model = sampleModel();
    const oneNamespaced: OntologyModel = {
      ...model,
      entityTypes: [
        { ...model.entityTypes[0]!, namespace: "https://graph-owl.dev/packs/gst#" },
        { ...model.entityTypes[1]!, namespace: null },
      ],
    };
    expect(namespacesIn(oneNamespaced)).toEqual(["https://graph-owl.dev/packs/gst#"]);
  });

  it("de-duplicates when several entities share a namespace", () => {
    const model = sampleModel();
    const same: OntologyModel = {
      ...model,
      entityTypes: [
        { ...model.entityTypes[0]!, namespace: "https://graph-owl.dev/packs/gst#" },
        { ...model.entityTypes[1]!, namespace: "https://graph-owl.dev/packs/gst#" },
      ],
    };
    expect(namespacesIn(same)).toEqual(["https://graph-owl.dev/packs/gst#"]);
  });
});

describe("filterModelByNamespace", () => {
  function mixedModel(): OntologyModel {
    const model = sampleModel();
    return {
      ...model,
      entityTypes: [
        { ...model.entityTypes[0]!, namespace: "https://graph-owl.dev/packs/gst#" },
        { ...model.entityTypes[1]!, namespace: "https://graph-owl.dev/packs/hospitality#" },
      ],
    };
  }

  it("returns the model unchanged when no namespace is selected — 'all namespaces'", () => {
    const model = mixedModel();
    expect(filterModelByNamespace(model, null)).toEqual(model);
  });

  it("keeps only entities in the selected namespace", () => {
    const filtered = filterModelByNamespace(mixedModel(), "https://graph-owl.dev/packs/gst#");
    expect(filtered.entityTypes.map((e) => e.id)).toEqual(["e1"]);
  });

  it("drops a relationship once either endpoint is filtered out", () => {
    // e1 (gst) --places--> e2 (hospitality): filtering to gst alone must not
    // leave a dangling edge pointing at a node that is no longer drawn.
    const filtered = filterModelByNamespace(mixedModel(), "https://graph-owl.dev/packs/gst#");
    expect(filtered.relationships).toEqual([]);
  });

  it("keeps a relationship whose both endpoints survive the filter", () => {
    const model = mixedModel();
    const bothGst: OntologyModel = {
      ...model,
      entityTypes: [
        { ...model.entityTypes[0]!, namespace: "https://graph-owl.dev/packs/gst#" },
        { ...model.entityTypes[1]!, namespace: "https://graph-owl.dev/packs/gst#" },
      ],
    };
    const filtered = filterModelByNamespace(bothGst, "https://graph-owl.dev/packs/gst#");
    expect(filtered.relationships).toHaveLength(1);
  });

  it("selecting a namespace nothing belongs to empties the picture rather than showing everything", () => {
    const filtered = filterModelByNamespace(mixedModel(), "https://graph-owl.dev/packs/nowhere#");
    expect(filtered.entityTypes).toEqual([]);
    expect(filtered.relationships).toEqual([]);
  });
});
