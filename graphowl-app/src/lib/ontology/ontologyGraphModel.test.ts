import { describe, expect, it } from "vitest";
import { ontologyEdgeStyle, ontologyLayoutOptions, ontologyNodeStyle, toOntologyG6Data } from "./ontologyGraphModel";
import { resolveEdgeStyle, resolveNodeStyle } from "../graph/graphModel";
import type { OntologyModel } from "./ontologyModel";
import { GRAPH_COLORS } from "../graph/graphColors";

function model(overrides?: Partial<OntologyModel>): OntologyModel {
  return {
    classes: [
      { id: "https://graph-owl.dev/packs/gst#PurchaseInvoice", name: "Purchase invoice", namespace: "https://graph-owl.dev/packs/gst#" },
      { id: "https://graph-owl.dev/packs/gst#Supplier", name: "Supplier", namespace: "https://graph-owl.dev/packs/gst#" },
    ],
    relationships: [
      {
        id: "https://graph-owl.dev/packs/gst#issuedBy",
        label: "issued by",
        from: "https://graph-owl.dev/packs/gst#PurchaseInvoice",
        to: "https://graph-owl.dev/packs/gst#Supplier",
      },
    ],
    properties: [],
    ...overrides,
  };
}

describe("turning the ontology model into G6 data", () => {
  it("emits one node per class and one edge per relationship", () => {
    const { nodes, edges } = toOntologyG6Data(model(), "dark");
    expect(nodes).toHaveLength(2);
    expect(edges).toHaveLength(1);
  });

  it("labels a node with the class's own resolved name", () => {
    const { nodes } = toOntologyG6Data(model(), "dark");
    const supplier = nodes.find((n) => n.id.endsWith("#Supplier"));
    expect(supplier?.data.label).toBe("Supplier");
  });

  it("labels an edge with the relationship's own name, directed from domain to range", () => {
    const { edges } = toOntologyG6Data(model(), "dark");
    expect(edges[0]).toMatchObject({
      source: "https://graph-owl.dev/packs/gst#PurchaseInvoice",
      target: "https://graph-owl.dev/packs/gst#Supplier",
      data: { label: "issued by" },
    });
  });

  /** Every class in one pack shares the pack's own namespace, so colouring
   *  by namespace gives one consistent colour per pack today — and a real,
   *  meaningful split the moment a second pack's classes are shown
   *  alongside it, rather than an arbitrary per-node colour with nothing
   *  behind it. */
  it("colours every node the same when every class shares one namespace", () => {
    const { nodes } = toOntologyG6Data(model(), "dark");
    expect(nodes[0]?.data.color).toBe(nodes[1]?.data.color);
  });

  it("colours two different namespaces differently", () => {
    const mixed = model({
      classes: [
        { id: "a", name: "A", namespace: "https://graph-owl.dev/packs/gst#" },
        { id: "b", name: "B", namespace: "https://graph-owl.dev/packs/hospitality#" },
      ],
    });
    const { nodes } = toOntologyG6Data(mixed, "dark");
    expect(nodes[0]?.data.color).not.toBe(nodes[1]?.data.color);
  });

  it("keeps a class with no relationships as an unconnected node", () => {
    const { nodes, edges } = toOntologyG6Data(
      model({
        classes: [
          { id: "a", name: "A", namespace: "https://graph-owl.dev/packs/gst#" },
        ],
        relationships: [],
      }),
      "dark",
    );
    expect(nodes).toHaveLength(1);
    expect(edges).toHaveLength(0);
  });
});

describe("the ontology graph's own layout tuning", () => {
  /** A wider spread than `graphModel.ts`'s `layoutOptions` — that was tuned
   *  against a 5–10 node instance neighbourhood; a whole pack's class
   *  diagram runs to 18+ classes, and reusing the tighter tuning packed
   *  them into an unreadably dense cluster in the middle of the canvas
   *  (checked live, not assumed). */
  it("spaces classes further apart than the instance-graph default", () => {
    const options = ontologyLayoutOptions() as {
      link: { distance: number };
      manyBody: { strength: number };
    };
    expect(options.link.distance).toBeGreaterThan(160);
    expect(options.manyBody.strength).toBeLessThan(-220);
  });

  it("stays on d3-force, the same layout the instance graph uses", () => {
    expect(ontologyLayoutOptions().type).toBe("d3-force");
  });
});

describe("the ontology graph's own node/edge weight", () => {
  const colors = GRAPH_COLORS.dark;
  const datum = { classes: "", label: "Supplier", color: "#3987e5" };

  /** An 18+ class diagram needs `fitView` to zoom out far more than the
   *  5–10 node instance neighbourhood `resolveNodeStyle`'s sizing was tuned
   *  for (checked live: at that zoom, the instance-graph's 34px/1px-stroke
   *  node became a handful of screen pixels — indistinguishable from
   *  "faded" even though every colour value was already full-contrast).
   *  Drawing ontology nodes measurably heavier than the instance default is
   *  what keeps them legible after that zoom-out, not a colour change. */
  it("draws a heavier node than the instance-graph default", () => {
    const base = resolveNodeStyle(datum, colors) as { size: number; lineWidth: number; fillOpacity: number };
    const heavier = ontologyNodeStyle(datum, colors) as { size: number; lineWidth: number; fillOpacity: number };
    expect(heavier.size).toBeGreaterThan(base.size);
    expect(heavier.lineWidth).toBeGreaterThan(base.lineWidth);
    expect(heavier.fillOpacity).toBeGreaterThan(base.fillOpacity);
  });

  it("draws a larger node label than the instance-graph default", () => {
    const base = resolveNodeStyle(datum, colors) as { labelFontSize: number };
    const heavier = ontologyNodeStyle(datum, colors) as { labelFontSize: number };
    expect(heavier.labelFontSize).toBeGreaterThan(base.labelFontSize);
  });

  it("still carries the class's own label and colour through, cascade untouched", () => {
    const style = ontologyNodeStyle(datum, colors) as { labelText: string; stroke: string };
    expect(style.labelText).toBe("Supplier");
    expect(style.stroke).toBe("#3987e5");
  });

  it("draws a heavier edge line than the instance-graph default", () => {
    const edgeDatum = { classes: "", label: "issued by" };
    const base = resolveEdgeStyle(edgeDatum, colors) as { lineWidth: number };
    const heavier = ontologyEdgeStyle(edgeDatum, colors) as { lineWidth: number };
    expect(heavier.lineWidth).toBeGreaterThan(base.lineWidth);
  });
});
