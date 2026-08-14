import { describe, expect, it } from "vitest";
import { brand } from "../theme";
import {
  edgeClasses,
  type Picture,
  WEBGL_THRESHOLD,
  edgeId,
  kindColor,
  layoutOptions,
  legendEntries,
  MAX_ZOOM,
  nodeClasses,
  resolveEdgeStyle,
  resolveNodeStyle,
  semanticTypeColor,
  toG6Data,
  visiblePicture,
  wantsWebgl,
} from "./graphModel";
import type { AssetKind } from "../api";

const colors = {
  text: "#0F172A",
  primary: "#14C3CF",
  border: "#E5E7EB",
  raised: "#FFFFFF",
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

  /** An edge to a node that is not in the picture happens legitimately — a
   *  diff can hold one whose far end authorization filtered out. */
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

  /** G6 draws labels through its own text shape, which — same as Cytoscape's
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
 *  name that matches every substring test while breaking every style
 *  rule — a failure invisible to an assertion phrased as "contains". */
function classesOf(node: Parameters<typeof nodeClasses>[0], p: Picture): string[] {
  return nodeClasses(node, p).split(" ").filter(Boolean);
}

describe("the classes a reader can act on", () => {
  it("separates classes so each one is a class", () => {
    const classes = classesOf(picture().nodes[0]!, picture());

    expect(classes).toContain("seed");
    expect(classes).toContain("unchanged");
    expect(classes.every((c) => !c.includes("seedunchanged"))).toBe(true);
  });

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

  it("carries the diff change so a removed node can still be drawn", () => {
    const p = picture({
      nodes: [{ id: "a", name: "gone", kind: "table", change: "removed" }],
    });

    expect(classesOf(p.nodes[0]!, p)).toContain("removed");
  });

  it("defaults to unchanged when there is no comparison", () => {
    expect(classesOf(picture().nodes[0]!, picture())).toContain("unchanged");
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

describe("the layout is deterministic", () => {
  /** Radial, focused on the seed, and never animated — same reasoning
   *  `cytoscape.ts` had for `breadthfirst`, ported to what G6 calls it. A
   *  force simulation settles somewhere slightly different every run, so
   *  the same neighbourhood never looks the same twice. */
  it("is radial, focused on the seed, and does not prevent overlap iteratively", () => {
    const options = layoutOptions("a");

    expect(options.type).toBe("radial");
    expect(options.focusNode).toBe("a");
    expect(options.preventOverlap).toBe(false);
  });

  it("produces identical options for the same seed", () => {
    expect(layoutOptions("a")).toEqual(layoutOptions("a"));
  });

  it("pins every option the layout's determinism rests on", () => {
    expect(layoutOptions("a")).toEqual({
      type: "radial",
      focusNode: "a",
      unitRadius: 90,
      preventOverlap: false,
    });
  });

  it("focuses on whichever node the canvas opened on", () => {
    expect(layoutOptions("z").focusNode).toBe("z");
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

  it("keeps the change class alongside", () => {
    const classes = edgeClasses({
      from: "a",
      to: "b",
      relationship: "feeds",
      derived: true,
      change: "added",
    }).split(" ");

    expect(classes).toContain("added");
    expect(classes).toContain("derived");
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
    const nodeStyle = resolveNodeStyle({ classes: "unchanged", label: "a" }, colors);
    const edgeStyle = resolveEdgeStyle({ classes: "unchanged", label: "feeds" }, colors);
    expect(nodeStyle.labelText).toBe("a");
    expect(edgeStyle.labelText).toBe("feeds");
  });

  it("draws a directed arrow on every edge", () => {
    const edgeStyle = resolveEdgeStyle({ classes: "unchanged", label: "feeds" }, colors);
    expect(edgeStyle.endArrow).toBe(true);
    expect(edgeStyle.endArrowType).toBe("triangle");
  });

  it("matches a derived edge's arrow to its own line colour, not the default", () => {
    const base = resolveEdgeStyle({ classes: "unchanged", label: "feeds" }, colors);
    const derived = resolveEdgeStyle({ classes: "unchanged derived", label: "feeds" }, colors);

    expect(derived.endArrowFill).toBe(derived.stroke);
    expect(derived.endArrowFill).not.toBe(base.endArrowFill);
  });

  it("gives the edge label a background so it survives a crossing line", () => {
    const edgeStyle = resolveEdgeStyle({ classes: "unchanged", label: "feeds" }, colors);
    expect(edgeStyle.labelBackground).toBe(true);
    expect(edgeStyle.labelBackgroundFill).toBe(colors.raised);
  });

  it("places the node label below the node, not on top of it", () => {
    const nodeStyle = resolveNodeStyle({ classes: "unchanged", label: "a" }, colors);
    expect(nodeStyle.labelPlacement).toBe("bottom");
  });

  it("caps how far a sparse graph can be zoomed to fill the canvas", () => {
    expect(MAX_ZOOM).toBe(2);
  });
});

describe("every class a reader can act on keeps its own drawing rule", () => {
  it("the seed stands out by size and weight", () => {
    const style = resolveNodeStyle({ classes: "unchanged seed", label: "a" }, colors);
    expect(style.size).toBe(26);
    expect(style.labelFontWeight).toBe("bold");
  });

  it("an expandable node is marked by a ring, not a colour", () => {
    const style = resolveNodeStyle({ classes: "unchanged expandable", label: "a" }, colors);
    expect(style.lineWidth).toBe(3);
    expect(style.stroke).toBe(colors.primary);
    expect(style.fillOpacity).toBe(0.35);
  });

  it("a node hiding neighbours is marked by a dashed border", () => {
    const style = resolveNodeStyle({ classes: "unchanged truncated", label: "a" }, colors);
    expect(style.lineWidth).toBe(3);
    expect(style.lineDash).toEqual([4, 2]);
    expect(style.stroke).toBe(colors.text);
  });

  it("a node whose kind is hidden by authorization loses its colour", () => {
    const style = resolveNodeStyle({ classes: "unchanged hidden-kind", color: "#2a78d6", label: "a" }, colors);
    expect(style.fill).toBe(colors.border);
  });

  it("a removed node is marked by opacity and a dashed border, not colour alone", () => {
    const style = resolveNodeStyle({ classes: "removed", label: "a" }, colors);
    expect(style.fillOpacity).toBe(0.4);
    expect(style.lineDash).toEqual([4, 2]);
    expect(style.stroke).toBe(colors.text);
  });

  it("a removed node is also marked by shape, at the element-type level", () => {
    const { nodes } = toG6Data({
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: "table", change: "removed" }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    expect(nodes[0]?.type).toBe("diamond");
  });

  it("an added node is marked by shape alone, at the element-type level", () => {
    const { nodes } = toG6Data({
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: "table", change: "added" }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    expect(nodes[0]?.type).toBe("star");
  });

  it("an ordinary node keeps the default circle shape", () => {
    const { nodes } = toG6Data(picture());
    expect(nodes[0]?.type).toBe("circle");
  });

  it("a derived edge is dashed and tinted, arrow included", () => {
    const style = resolveEdgeStyle({ classes: "unchanged derived", label: "feeds" }, colors);
    expect(style.lineDash).toEqual([4, 2]);
    expect(style.stroke).toBe(brand.cyan400);
    expect(style.endArrowFill).toBe(brand.cyan400);
  });

  it("a removed edge is dashed in the text colour, not the line colour", () => {
    const style = resolveEdgeStyle({ classes: "removed", label: "feeds" }, colors);
    expect(style.lineDash).toEqual([4, 2]);
    expect(style.stroke).toBe(colors.text);
  });

  it("an added edge is thicker and in the primary colour", () => {
    const style = resolveEdgeStyle({ classes: "added", label: "feeds" }, colors);
    expect(style.lineWidth).toBe(2);
    expect(style.stroke).toBe(colors.primary);
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
