import { describe, expect, it } from "vitest";
import {
  edgeClasses,
  edgeLegendEntries,
  type Picture,
  WEBGL_THRESHOLD,
  edgeId,
  kindColor,
  layoutOptions,
  legendEntries,
  MAX_ZOOM,
  nodeClasses,
  nodeGlyph,
  shortLabel,
  MAX_LABEL_CHARS,
  resolveEdgeStyle,
  resolveNodeStyle,
  semanticTypeColor,
  toG6Data,
  withEntryPositions,
  visiblePicture,
  wantsWebgl,
} from "./graphModel";
import type { AssetKind } from "../api";

const colors = {
  text: "#0F172A",
  primary: "#14C3CF",
  border: "#E5E7EB",
  raised: "#FFFFFF",
  inferred: "#a3641a",
};

function picture(overrides?: Partial<Picture>): Picture {
  return {
    seedId: "a",
    nodes: [
      { id: "a", name: "upi_transactions", kind: "table" },
      { id: "b", name: "amount", kind: "column" },
    ],
    edges: [{ from: "a", to: "b", relationship: "contains" }],
    expanded: ["a"],
    truncatedAt: [],
    ...overrides,
  };
}

describe("turning the model into G6 data", () => {
  it("emits a node per node and an edge per edge", () => {
    const { nodes, edges } = toG6Data(picture());

    expect(nodes).toHaveLength(2);
    expect(edges).toHaveLength(1);
  });

  it("drops an edge whose endpoint is not in the picture", () => {
    const { nodes, edges } = toG6Data(
      picture({ edges: [{ from: "a", to: "ghost", relationship: "feeds" }] }),
    );

    expect(edges).toHaveLength(0);
    expect(nodes).toHaveLength(2);
  });

  /** And the negative: a *present* endpoint must not be dropped, or the
   *  filter above would be satisfied by drawing no edges at all. */
  it("keeps an edge whose endpoints are both present", () => {
    expect(toG6Data(picture()).edges).toHaveLength(1);
  });

  it("carries the node name as the label a reader sees", () => {
    const node = toG6Data(picture()).nodes.find((n) => n.id === "a");

    expect(node?.data.label).toBe("upi_transactions");
  });

  /** G6 draws labels through its own text shape, which — like any
   *  `<canvas>` — has no `dir` attribute, so a right-to-left name needs to
   *  arrive already in visual order (`bidiLabel.ts`'s own doc comment). Real
   *  Hebrew text, not a placeholder, matching this project's own standing
   *  rule for bidi RED tests. */
  it("repositions a right-to-left node name's runs for canvas rendering rather than passing it through unchanged", () => {
    const rtlName = "לקוח_orders";
    const graph = picture({ nodes: [{ id: "a", name: rtlName, kind: "table" }] });

    const node = toG6Data(graph).nodes.find((n) => n.id === "a");

    expect(node?.data.label).not.toBe(rtlName);
    expect(node?.data.label).toBe("orders_לקוח");
  });
});

describe("edge identity", () => {
  /** `a contains b` and `a feeds b` are two facts about one pair. An id
   *  built from the endpoints alone would collide the two. */
  it("distinguishes two relationships between the same pair", () => {
    expect(edgeId({ from: "a", to: "b", relationship: "contains" })).not.toBe(
      edgeId({ from: "a", to: "b", relationship: "feeds" }),
    );
  });

  it("is stable for the same fact", () => {
    const edge = { from: "a", to: "b", relationship: "contains" };
    expect(edgeId(edge)).toBe(edgeId({ ...edge }));
  });

  it("distinguishes direction", () => {
    expect(edgeId({ from: "a", to: "b", relationship: "feeds" })).not.toBe(
      edgeId({ from: "b", to: "a", relationship: "feeds" }),
    );
  });

  it("keeps both edges of a pair in the data", () => {
    const { edges } = toG6Data(
      picture({
        edges: [
          { from: "a", to: "b", relationship: "contains" },
          { from: "a", to: "b", relationship: "feeds" },
        ],
      }),
    );

    const ids = edges.map((e) => e.id);
    expect(new Set(ids).size).toBe(2);
  });
});

/** Asserted as a *split list*, never with `toContain` against the raw
 *  string. Joining classes without a separator yields one garbage class
 *  name that matches every substring test while breaking every style rule —
 *  a failure invisible to an assertion phrased as "contains". */
function classesOf(node: Parameters<typeof nodeClasses>[0], p: Picture): string[] {
  return nodeClasses(node, p).split(" ").filter(Boolean);
}

describe("the classes a reader can act on", () => {
  it("marks the seed", () => {
    expect(classesOf(picture().nodes[0]!, picture())).toContain("seed");
  });

  it("marks only the seed", () => {
    const p = picture();
    expect(classesOf(p.nodes[1]!, p)).not.toContain("seed");
  });

  it("marks an unexpanded node as expandable, and an expanded one not", () => {
    const p = picture();
    expect(classesOf(p.nodes[1]!, p)).toContain("expandable");
    expect(classesOf(p.nodes[0]!, p)).not.toContain("expandable");
  });

  it("marks the node that is hiding neighbours", () => {
    const p = picture({ truncatedAt: ["b"] });

    expect(classesOf(p.nodes[1]!, p)).toContain("truncated");
    expect(classesOf(p.nodes[0]!, p)).not.toContain("truncated");
  });

  it("marks a node whose kind is hidden by authorization", () => {
    const p = picture({ nodes: [{ id: "a", name: "?", kind: null }] });

    expect(classesOf(p.nodes[0]!, p)).toContain("hidden-kind");
  });

  it("does not mark an ordinary node as hidden", () => {
    expect(classesOf(picture().nodes[1]!, picture())).not.toContain("hidden-kind");
  });

  it("does not mark a kind-null node hidden when it has a semantic type to colour it by instead", () => {
    const p = picture({
      nodes: [{ id: "a", name: "g1-INV-1008", kind: null, semanticType: "Gstr1Invoice" }],
    });

    expect(classesOf(p.nodes[0]!, p)).not.toContain("hidden-kind");
  });

  it("still marks a kind-null node hidden when it has no semantic type either", () => {
    const p = picture({
      nodes: [{ id: "a", name: "?", kind: null, semanticType: null }],
    });

    expect(classesOf(p.nodes[0]!, p)).toContain("hidden-kind");
  });
});

describe("the force-directed layout", () => {
  it("is d3-force, matching the reference example", () => {
    expect(layoutOptions().type).toBe("d3-force");
  });

  /** `link`/`manyBody`/`collide` as nested objects, matching G6's own
   *  published example (`examples/layout/force-directed/#d3-force`) —
   *  `@antv/layout`'s `D3ForceLayoutOptions` also accepts a flatter
   *  `linkDistance`/`nodeStrength`/`preventOverlap` shape, but the nested one
   *  is what the reference example and its docs actually show, so it is what
   *  this matches rather than an equivalent the docs never demonstrate. */
  it("sizes collisions by an explicit radius, matching the reference shape", () => {
    const options = layoutOptions() as { collide?: { radius?: number } };
    expect(options.collide?.radius).toBe(90);
  });

  /** **`d3-force` is deterministic here, verified against its own source, not
   *  assumed from its reputation.** A force simulation is popularly believed
   *  to settle differently every run, and it can — `Math.random()` in the
   *  wrong place would make it so. `d3-force`'s own `initializeNodes` seeds
   *  each node's starting position from its *index* in the array
   *  (`initialRadius * sqrt(0.5 + i)`, `angle = i * initialAngle`), and every
   *  force that wants randomness draws from an `lcg()` seeded with the fixed
   *  constant `s = 1` — not the clock, not `Math.random`. Given the same
   *  node order, the same edges and the same options, every run produces the
   *  same simulation. Confirmed empirically too — see `GraphCanvas.tsx`'s
   *  layout effect for the double-load comparison, done against each node's
   *  `x`/`y` rather than a rendered-pixel hash, which canvas anti-aliasing
   *  does not guarantee to agree even when the underlying layout does. */
  it("pins every option the layout's determinism rests on", () => {
    expect(layoutOptions()).toEqual({
      type: "d3-force",
      link: { distance: 160 },
      manyBody: { strength: -220 },
      collide: { radius: 90 },
      alphaDecay: 0.05,
    });
  });

  it("produces identical options on every call", () => {
    expect(layoutOptions()).toEqual(layoutOptions());
  });
});

describe("choosing a renderer", () => {
  it("stays on canvas for a small neighbourhood", () => {
    expect(wantsWebgl(0)).toBe(false);
    expect(wantsWebgl(6)).toBe(false);
    expect(wantsWebgl(WEBGL_THRESHOLD - 1)).toBe(false);
  });

  it("switches on at the threshold and above", () => {
    expect(wantsWebgl(WEBGL_THRESHOLD)).toBe(true);
    expect(wantsWebgl(10_000)).toBe(true);
  });

  it("sits well below the interactivity budget", () => {
    expect(WEBGL_THRESHOLD).toBeLessThan(1_000);
  });
});

describe("a derived edge is drawn differently", () => {
  it("carries a class an asserted edge does not", () => {
    const derived = edgeClasses({ from: "a", to: "b", relationship: "feeds", derived: true });
    const asserted = edgeClasses({ from: "a", to: "b", relationship: "feeds", derived: false });

    expect(derived.split(" ")).toContain("derived");
    expect(asserted.split(" ")).not.toContain("derived");
  });

  it("treats an absent flag as asserted", () => {
    expect(edgeClasses({ from: "a", to: "b", relationship: "feeds" }).split(" ")).not.toContain(
      "derived",
    );
  });

  it("reaches the data the canvas draws", () => {
    const { edges } = toG6Data({
      seedId: "a",
      nodes: [
        { id: "a", name: "a", kind: "table" },
        { id: "b", name: "b", kind: "table" },
      ],
      edges: [{ from: "a", to: "b", relationship: "feeds", derived: true }],
      expanded: ["a", "b"],
      truncatedAt: [],
    });

    expect(edges[0]?.data.classes.split(" ")).toContain("derived");
  });
});

describe("the style a reader can actually see", () => {
  it("draws the relationship name on every edge, not only the node name", () => {
    const nodeStyle = resolveNodeStyle({ classes: "", label: "a" }, colors);
    const edgeStyle = resolveEdgeStyle({ classes: "", label: "feeds" }, colors);
    expect(nodeStyle.labelText).toBe("a");
    expect(edgeStyle.labelText).toBe("feeds");
  });

  it("draws a directed arrow on every edge", () => {
    const edgeStyle = resolveEdgeStyle({ classes: "", label: "feeds" }, colors);
    expect(edgeStyle.endArrow).toBe(true);
    expect(edgeStyle.endArrowType).toBe("triangle");
  });

  it("matches a derived edge's arrow to its own line colour, not the default", () => {
    const base = resolveEdgeStyle({ classes: "", label: "feeds" }, colors);
    const derived = resolveEdgeStyle({ classes: "derived", label: "feeds" }, colors);

    expect(derived.endArrowFill).toBe(derived.stroke);
    expect(derived.endArrowFill).not.toBe(base.endArrowFill);
  });

  it("gives the edge label a background so it survives a crossing line", () => {
    const edgeStyle = resolveEdgeStyle({ classes: "", label: "feeds" }, colors);
    expect(edgeStyle.labelBackground).toBe(true);
    expect(edgeStyle.labelBackgroundFill).toBe(colors.raised);
  });

  it("places the node label below the node, not on top of it", () => {
    const nodeStyle = resolveNodeStyle({ classes: "", label: "a" }, colors);
    expect(nodeStyle.labelPlacement).toBe("bottom");
  });

  it("caps how far a sparse graph can be zoomed to fill the canvas", () => {
    expect(MAX_ZOOM).toBe(2);
  });
});

describe("a node is an outlined circle with a lettered glyph, not a solid dot — matches the delivered mockup", () => {
  it("draws the node as a ring in the kind's colour, not a solid fill of it", () => {
    const style = resolveNodeStyle({ classes: "", label: "a", color: "#2a78d6" }, colors);
    expect(style.stroke).toBe("#2a78d6");
    expect(style.fill).not.toBe("#2a78d6");
  });

  it("falls back to the border colour for the ring when the node carries none", () => {
    const style = resolveNodeStyle({ classes: "", label: "a" }, colors);
    expect(style.stroke).toBe(colors.border);
  });

  it("letters the glyph inside the ring in the same colour as the ring itself", () => {
    const style = resolveNodeStyle({ classes: "", label: "a", glyph: "TBL", color: "#eda100" }, colors);
    expect(style.icon).toBe(true);
    expect(style.iconText).toBe("TBL");
    expect(style.iconFill).toBe("#eda100");
  });
});

describe("every class a reader can act on keeps its own drawing rule", () => {
  it("the seed stands out by size and weight", () => {
    const style = resolveNodeStyle({ classes: "seed", label: "a" }, colors);
    expect(style.size).toBe(44);
    expect(style.labelFontWeight).toBe("bold");
  });

  /** **Expandable is no longer drawn as a heavy ring.** Nearly every node in
   *  a freshly opened neighbourhood is expandable, so the marker fired almost
   *  everywhere and the canvas read as uniformly heavy — the ring is 1px in
   *  the design, and a 3px default swamped it. The affordance moved to the
   *  hover controls, which say what the click will do instead of leaving the
   *  reader to infer it from a border. */
  it("draws an expandable node no heavier than any other", () => {
    const base = resolveNodeStyle({ classes: "", label: "a" }, colors);
    const style = resolveNodeStyle({ classes: "expandable", label: "a" }, colors);
    expect(style.lineWidth).toBe(base.lineWidth);
  });

  it("keeps the ring hairline, as the design draws it", () => {
    expect(resolveNodeStyle({ classes: "", label: "a" }, colors).lineWidth).toBe(1);
    expect(resolveNodeStyle({ classes: "seed", label: "a" }, colors).lineWidth).toBe(2);
  });

  it("a node hiding neighbours is marked by a dashed border", () => {
    const style = resolveNodeStyle({ classes: "truncated", label: "a" }, colors);
    expect(style.lineWidth).toBe(2);
    expect(style.lineDash).toEqual([4, 2]);
    expect(style.stroke).toBe(colors.text);
  });

  it("a node whose kind is hidden by authorization loses its ring colour and its glyph colour", () => {
    const style = resolveNodeStyle({ classes: "hidden-kind", color: "#2a78d6", label: "a", glyph: "?" }, colors);
    expect(style.stroke).toBe(colors.border);
    expect(style.iconFill).toBe(colors.border);
  });

  it("an ordinary node keeps the default circle shape", () => {
    const { nodes } = toG6Data(picture());
    expect(nodes[0]?.type).toBe("circle");
  });

  it("a derived edge is dashed and tinted, arrow included", () => {
    const style = resolveEdgeStyle({ classes: "derived", label: "feeds" }, colors);
    expect(style.lineDash).toEqual([6, 5]);
    expect(style.stroke).toBe(colors.inferred);
    expect(style.endArrowFill).toBe(colors.inferred);
  });
});

describe("the glyph lettered inside a node — the node's own name, shortened, not a kind/type lookup", () => {
  it("takes the first word of the name, matching the mockup's own INV-1024 → INV", () => {
    expect(nodeGlyph("INV-1024")).toBe("INV");
  });

  it("stops at a separator rather than reading past it", () => {
    expect(nodeGlyph("orders-service")).toBe("ORD");
    expect(nodeGlyph("gst return jul")).toBe("GST");
    expect(nodeGlyph("erp_master")).toBe("ERP");
  });

  it("uppercases, and takes no more than three characters", () => {
    expect(nodeGlyph("maharashtra")).toBe("MAH");
  });

  it("uses the whole name when it is already three characters or fewer", () => {
    expect(nodeGlyph("db")).toBe("DB");
  });

  it("falls back to a neutral placeholder for a name with no letters or digits at all", () => {
    expect(nodeGlyph("---")).toBe("?");
    expect(nodeGlyph("")).toBe("?");
  });

  it("reaches the data the canvas draws, from the node's own name — not its kind", () => {
    const { nodes } = toG6Data({
      seedId: "a",
      nodes: [{ id: "a", name: "orders-service", kind: "table" }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    expect(nodes[0]?.data.glyph).toBe("ORD");
  });
});

describe("temporarily hiding a node — Plan 114 Slice B", () => {
  const aContainsB = { from: "a", to: "b", relationship: "contains" };
  const aContainsC = { from: "a", to: "c", relationship: "contains" };
  const threeNode: Picture = {
    seedId: "a",
    nodes: [
      { id: "a", name: "a", kind: "table" },
      { id: "b", name: "b", kind: "column" },
      { id: "c", name: "c", kind: "column" },
    ],
    edges: [aContainsB, aContainsC],
    expanded: ["a"],
    truncatedAt: [],
  };

  it("removes a hidden node and drops its incident edges from what reaches the data", () => {
    const { nodes, edges } = toG6Data(visiblePicture(threeNode, new Set(["b"])));

    expect(nodes.some((n) => n.id === "b")).toBe(false);
    expect(edges.some((e) => e.id === edgeId(aContainsB))).toBe(false);
    expect(nodes.some((n) => n.id === "c")).toBe(true);
    expect(edges.some((e) => e.id === edgeId(aContainsC))).toBe(true);
  });

  it("hiding a node with no edges of its own leaves the rest of the picture unchanged", () => {
    const isolated: Picture = {
      ...threeNode,
      nodes: [...threeNode.nodes, { id: "d", name: "d", kind: "column" }],
    };

    const data = toG6Data(visiblePicture(isolated, new Set(["d"])));

    expect(data).toEqual(toG6Data(threeNode));
  });

  it("showing all — an empty hidden set — restores exactly the original picture", () => {
    expect(visiblePicture(threeNode, new Set())).toEqual(threeNode);
  });
});

describe("colouring a node by its kind — Plan 114 Slice D", () => {
  const KINDS: readonly AssetKind[] = ["service", "database", "schema", "table", "column"];

  it("gives every kind its own colour, none repeated", () => {
    const colours = KINDS.map((kind) => kindColor(kind, "light", colors));
    expect(new Set(colours).size).toBe(KINDS.length);
  });

  it("falls back to the border colour for a hidden kind, matching hidden-kind's style", () => {
    expect(kindColor(null, "light", colors)).toBe(colors.border);
  });

  it("uses a different set of hexes for dark mode, not the same colours regardless of theme", () => {
    for (const kind of KINDS) {
      expect(kindColor(kind, "dark", colors)).not.toBe(kindColor(kind, "light", colors));
    }
  });

  it("resolves each kind to its own validated hex, per theme", () => {
    expect(kindColor("service", "light", colors)).toBe("#2a78d6");
    expect(kindColor("database", "light", colors)).toBe("#eb6834");
    expect(kindColor("schema", "light", colors)).toBe("#1baf7a");
    expect(kindColor("table", "light", colors)).toBe("#eda100");
    expect(kindColor("column", "light", colors)).toBe("#e87ba4");
    expect(kindColor("service", "dark", colors)).toBe("#3987e5");
    expect(kindColor("database", "dark", colors)).toBe("#d95926");
    expect(kindColor("schema", "dark", colors)).toBe("#199e70");
    expect(kindColor("table", "dark", colors)).toBe("#c98500");
    expect(kindColor("column", "dark", colors)).toBe("#d55181");
  });

  it("carries a kind's colour into the node data toG6Data builds", () => {
    const { nodes } = toG6Data(
      { seedId: "a", nodes: [{ id: "a", name: "a", kind: "table" }], edges: [], expanded: ["a"], truncatedAt: [] },
      "light",
    );
    expect(nodes[0]?.data.color).toBe(kindColor("table", "light", colors));
  });

  it("leaves a hidden-kind node's data without a colour, so only resolveNodeStyle's hidden-kind branch decides it", () => {
    const { nodes } = toG6Data({
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: null }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    const data = nodes[0]?.data;
    expect(data && "color" in data).toBe(false);
  });
});

describe("the legend — Plan 114 Slice D", () => {
  const catalogPicture: Picture = {
    seedId: "a",
    nodes: [
      { id: "a", name: "a", kind: "table" },
      { id: "b", name: "b", kind: "column" },
      { id: "c", name: "c", kind: "table" },
    ],
    edges: [],
    expanded: ["a"],
    truncatedAt: [],
  };

  it("lists only the kinds actually present in the picture, in fixed slot order", () => {
    const entries = legendEntries(catalogPicture, "light", colors);
    expect(entries.map((e) => e.key)).toEqual(["table", "column"]);
  });

  it("recognises every one of the five kinds, in their fixed order", () => {
    const allFive: Picture = {
      ...catalogPicture,
      nodes: (["column", "service", "table", "database", "schema"] as const).map((kind, i) => ({
        id: `n${i}`,
        name: `n${i}`,
        kind,
      })),
    };
    expect(legendEntries(allFive, "light", colors).map((e) => e.key)).toEqual([
      "service",
      "database",
      "schema",
      "table",
      "column",
    ]);
  });

  it("a kind-null node contributes no AssetKind entry of its own", () => {
    const mixed: Picture = {
      ...catalogPicture,
      nodes: [...catalogPicture.nodes, { id: "d", name: "d", kind: null }],
    };
    expect(legendEntries(mixed, "light", colors).map((e) => e.key)).toEqual(["table", "column"]);
  });

  it("each entry's colour is exactly what a node of that kind would be drawn in", () => {
    for (const entry of legendEntries(catalogPicture, "dark", colors)) {
      expect(entry.color).toBe(kindColor(entry.key as AssetKind, "dark", colors));
    }
  });

  it("labels read for a human, not the raw kind string", () => {
    const table = legendEntries(catalogPicture, "light", colors).find((e) => e.key === "table");
    expect(table?.label).toBe("Table");
  });

  it("an empty picture legends nothing", () => {
    expect(legendEntries({ ...catalogPicture, nodes: [] }, "light", colors)).toEqual([]);
  });
});

describe("colouring a node by its pack-declared semantic type — Plan 114 Slice F", () => {
  it("is deterministic — the same type always resolves to the same colour", () => {
    expect(semanticTypeColor("Gstr2bInvoice", "light")).toBe(
      semanticTypeColor("Gstr2bInvoice", "light"),
    );
  });

  it("resolves this exact type to its actual hashed slot, not just a member of the palette", () => {
    expect(semanticTypeColor("Gstr2bInvoice", "light")).toBe("#eda100");
  });

  it("resolves to one of the validated 8-slot hexes for that mode", () => {
    const LIGHT_HEXES = [
      "#2a78d6",
      "#eb6834",
      "#1baf7a",
      "#eda100",
      "#e87ba4",
      "#008300",
      "#4a3aa7",
      "#e34948",
    ];
    expect(LIGHT_HEXES).toContain(semanticTypeColor("Supplier", "light"));
  });

  it("every one of the 8 slots resolves to its own validated hex, in both modes", () => {
    const bySlot = ["H", "A", "B", "C", "D", "E", "F", "G"];
    expect(bySlot.map((s) => semanticTypeColor(s, "light"))).toEqual([
      "#2a78d6",
      "#eb6834",
      "#1baf7a",
      "#eda100",
      "#e87ba4",
      "#008300",
      "#4a3aa7",
      "#e34948",
    ]);
    expect(bySlot.map((s) => semanticTypeColor(s, "dark"))).toEqual([
      "#3987e5",
      "#d95926",
      "#199e70",
      "#c98500",
      "#d55181",
      "#008300",
      "#9085e9",
      "#e66767",
    ]);
  });

  it("uses a different hex set for dark mode", () => {
    const DARK_HEXES = [
      "#3987e5",
      "#d95926",
      "#199e70",
      "#c98500",
      "#d55181",
      "#008300",
      "#9085e9",
      "#e66767",
    ];
    expect(DARK_HEXES).toContain(semanticTypeColor("Supplier", "dark"));
  });

  it("carries the type into toG6Data's node data when the kind is null", () => {
    const { nodes } = toG6Data(
      {
        seedId: "a",
        nodes: [{ id: "a", name: "a", kind: null, semanticType: "Gstr2bInvoice" }],
        edges: [],
        expanded: ["a"],
        truncatedAt: [],
      },
      "light",
    );
    expect(nodes[0]?.data.color).toBe(semanticTypeColor("Gstr2bInvoice", "light"));
  });

  it("leaves an untyped, kind-null node's data without a colour, same as before", () => {
    const { nodes } = toG6Data({
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: null, semanticType: null }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    expect(nodes[0]?.data && "color" in nodes[0]!.data).toBe(false);
  });

  it("the legend lists semantic types actually present, after any AssetKind entries", () => {
    const picture: Picture = {
      seedId: "a",
      nodes: [
        { id: "a", name: "a", kind: null, semanticType: "Gstr2bInvoice" },
        { id: "b", name: "b", kind: null, semanticType: "Supplier" },
        { id: "c", name: "c", kind: null, semanticType: "Gstr2bInvoice" },
      ],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    };

    const entries = legendEntries(picture, "light", colors);
    expect(entries.map((e) => e.key)).toEqual(["Gstr2bInvoice", "Supplier"]);
    expect(entries.find((e) => e.key === "Gstr2bInvoice")?.color).toBe(
      semanticTypeColor("Gstr2bInvoice", "light"),
    );
  });

  it("a node with a real kind never earns a semantic-type entry too, even if semanticType is set", () => {
    const picture: Picture = {
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: "table", semanticType: "Gstr2bInvoice" }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    };
    expect(legendEntries(picture, "light", colors).map((e) => e.key)).toEqual(["table"]);
  });
});

/** The **provenance key** — what a line's drawing means.
 *
 *  Distinct from `legendEntries` above, which names the *node* kinds present.
 *  A GST subject has no catalog `kind` and no `semanticType`, so that legend
 *  is empty for exactly the data the console spends most of its time on, and
 *  the canvas ends up with no key at all. This one describes the edge
 *  encoding `resolveEdgeStyle` actually applies, so it is present whenever
 *  anything is drawn. */
describe("the edge provenance key", () => {
  it("names both edge states the canvas can draw, so the key explains the drawing", () => {
    expect(edgeLegendEntries(picture(), colors).map((e) => e.key)).toEqual([
      "asserted",
      "inferred",
    ]);
  });

  /** A key describes an encoding, not a census — but a picture with no edges
   *  at all has no encoding to explain, and a key floating over an edgeless
   *  canvas is furniture. */
  it("keys nothing when there are no edges to explain", () => {
    expect(edgeLegendEntries(picture({ edges: [] }), colors)).toEqual([]);
  });

  /** **The key must not move under the reader.** An entry that appeared only
   *  once a derived edge arrived would make the legend reflow mid-expansion,
   *  and the reader is using its position to read the canvas. */
  it("keys both states even when every edge present is asserted", () => {
    const allAsserted = picture({
      edges: [{ from: "a", to: "b", relationship: "contains", derived: false }],
    });
    expect(edgeLegendEntries(allAsserted, colors).map((e) => e.key)).toEqual([
      "asserted",
      "inferred",
    ]);
  });

  it("draws each state exactly as an edge of that state is drawn", () => {
    const [asserted, inferred] = edgeLegendEntries(picture(), colors);
    expect(asserted?.color).toBe(colors.border);
    expect(asserted?.dashed).toBe(false);
    expect(inferred?.color).toBe(colors.inferred);
    expect(inferred?.dashed).toBe(true);
  });

  it("labels read for a human", () => {
    expect(edgeLegendEntries(picture(), colors).map((e) => e.label)).toEqual([
      "Asserted",
      "Inferred",
    ]);
  });
});

/** The type caption under a node's name — `gst:Supplier` beneath
 *  "Patel Chemicals & Co". Without it a reader can see that two nodes are
 *  coloured differently but not what either colour *is*. */
describe("the node type caption", () => {
  it("captions a node with its resolved type", () => {
    const typed = picture({
      nodes: [{ id: "a", name: "Patel Chemicals", kind: null, semanticType: "gst:Supplier" }],
      edges: [],
    });
    expect(toG6Data(typed).nodes[0]?.data.caption).toBe("gst:Supplier");
  });

  /** A catalog asset's kind is the same class of fact, and reads the same way
   *  under the name. */
  it("captions a catalog asset with its kind", () => {
    const typed = picture({ nodes: [{ id: "a", name: "orders", kind: "table" }], edges: [] });
    expect(toG6Data(typed).nodes[0]?.data.caption).toBe("table");
  });

  /** **No caption rather than an invented one.** A node the graph could not
   *  type gets nothing under its name — a placeholder like "unknown" reads as
   *  a class the ontology declares. */
  it("captions nothing when the node has neither kind nor type", () => {
    const untyped = picture({ nodes: [{ id: "a", name: "a", kind: null }], edges: [] });
    expect(toG6Data(untyped).nodes[0]?.data.caption).toBeUndefined();
  });

  it("draws the caption under the name, in the muted colour", () => {
    const style = resolveNodeStyle(
      { classes: "", label: "Patel Chemicals", caption: "gst:Supplier" },
      colors,
    );
    expect(style["labelText"]).toBe("Patel Chemicals\ngst:Supplier");
  });

  it("draws just the name when there is no caption", () => {
    expect(resolveNodeStyle({ classes: "", label: "Patel" }, colors)["labelText"]).toBe("Patel");
  });
});

/** Identifier-shaped names are long, and a canvas has no room for them.
 *  `books-19AABCP8087C1ZV-INV-MAR-006` at full length overlaps its
 *  neighbours until the picture is unreadable. */
describe("shortening a label for the canvas", () => {
  it("leaves a name that already fits completely alone", () => {
    expect(shortLabel("Patel Chemicals & Co")).toBe("Patel Chemicals & Co");
  });

  /** **Middle-ellipsis, not a truncated tail.** These ids carry meaning at
   *  both ends — the family at the front (`books-`, `payments-`) and the
   *  document number at the back (`INV-MAR-006`) — and two invoices for the
   *  same supplier differ only in the tail, so cutting it makes distinct
   *  nodes read as identical. */
  it("keeps both ends of a long identifier and elides the middle", () => {
    const short = shortLabel("books-19AABCP8087C1ZV-INV-MAR-006");
    expect(short.startsWith("books-")).toBe(true);
    expect(short.endsWith("INV-MAR-006")).toBe(true);
    expect(short).toContain("…");
  });

  it("never returns more than the budget", () => {
    expect(shortLabel("books-19AABCP8087C1ZV-INV-MAR-006").length).toBeLessThanOrEqual(
      MAX_LABEL_CHARS,
    );
  });

  /** Two invoices differing only in their tail must stay visibly different —
   *  the failure this shortening exists to avoid. */
  it("keeps two ids that differ only at the end distinguishable", () => {
    expect(shortLabel("books-19AABCP8087C1ZV-INV-MAR-006")).not.toBe(
      shortLabel("books-19AABCP8087C1ZV-INV-MAR-007"),
    );
  });

  it("shortens the name the canvas actually draws", () => {
    const long = picture({
      nodes: [{ id: "a", name: "books-19AABCP8087C1ZV-INV-MAR-006", kind: null }],
      edges: [],
    });
    expect(toG6Data(long).nodes[0]?.data.label).toBe(
      shortLabel("books-19AABCP8087C1ZV-INV-MAR-006"),
    );
  });
});

/** The three line treatments the design uses, and what each one means. */
describe("edge treatments", () => {
  const asserted = resolveEdgeStyle({ classes: "", label: "issuedBy" }, colors);
  const inferred = resolveEdgeStyle({ classes: "derived", label: "locatedIn" }, colors);

  it("draws an asserted edge as a solid hairline", () => {
    expect(asserted.lineDash).toBeUndefined();
    expect(asserted.stroke).toBe(colors.border);
  });

  /** Dashed *and* recoloured: a state a reader acts on must survive somebody
   *  who cannot tell two hues apart, so the dash carries the meaning and the
   *  colour only reinforces it. */
  it("draws an inferred edge dashed and in the inferred colour", () => {
    expect(inferred.lineDash).toEqual([6, 5]);
    expect(inferred.stroke).toBe(colors.inferred);
  });

  it("labels the edge with its relationship, on its own plate", () => {
    expect(asserted.labelText).toBe("issuedBy");
    expect(asserted.labelBackground).toBe(true);
    expect(asserted.labelFontSize).toBe(9.5);
  });
});

describe("seeding a newly expanded node's starting position", () => {
  const before = toG6Data(
    picture({
      nodes: [
        { id: "a", name: "a", kind: "table" },
        { id: "b", name: "b", kind: "column" },
      ],
      edges: [{ from: "a", to: "b", relationship: "contains" }],
    }),
  );

  const after = toG6Data(
    picture({
      nodes: [
        { id: "a", name: "a", kind: "table" },
        { id: "b", name: "b", kind: "column" },
        { id: "c", name: "c", kind: "table" },
      ],
      edges: [
        { from: "a", to: "b", relationship: "contains" },
        { from: "b", to: "c", relationship: "feeds" },
      ],
    }),
  );

  /** **This is what makes an expansion read as "bubbling out of" the node the
   *  reader clicked, instead of appearing wherever the layout's own
   *  index-based placement happens to land it.** Co-locating a brand-new
   *  node with the node it was reached from — rather than leaving it
   *  unpositioned — gives the force simulation a real starting point next to
   *  its neighbour, and gives G6 an "enter" position to tween outward from
   *  once the layout settles it. Without a seed, a new node can start on the
   *  opposite side of the canvas from the thing that just grew it, and jump
   *  there rather than growing out of it. */
  it("places a node new to this picture at the anchor's position", () => {
    const seeded = withEntryPositions(after, before, "a", { x: 100, y: 200 });
    const c = seeded.nodes.find((n) => n.id === "c");
    expect(c?.style).toEqual({ x: 100, y: 200 });
  });

  it("leaves a node already present in the previous picture unseeded", () => {
    const seeded = withEntryPositions(after, before, "a", { x: 100, y: 200 });
    const b = seeded.nodes.find((n) => n.id === "b");
    expect(b?.style).toBeUndefined();
  });

  it("does not disturb edges", () => {
    const seeded = withEntryPositions(after, before, "a", { x: 100, y: 200 });
    expect(seeded.edges).toEqual(after.edges);
  });

  it("seeds nothing when nothing is new", () => {
    const seeded = withEntryPositions(before, before, "a", { x: 100, y: 200 });
    expect(seeded.nodes.every((n) => n.style === undefined)).toBe(true);
  });
});
