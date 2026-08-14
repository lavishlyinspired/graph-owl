import { describe, expect, it } from "vitest";
import { brand } from "../theme";
import {
  edgeClasses,
  type Picture,
  WEBGL_THRESHOLD,
  edgeId,
  graphStyle,
  kindColor,
  layoutOptions,
  legendEntries,
  MAX_ZOOM,
  nodeClasses,
  toElements,
  visiblePicture,
  wantsWebgl,
} from "./cytoscape";
import type { AssetKind } from "../api";

const colors = {
  text: "#0F172A",
  primary: "#14C3CF",
  border: "#E5E7EB",
  raised: "#FFFFFF",
};

function ruleFor(selector: string) {
  const rule = graphStyle(colors).find((r) => r.selector === selector);
  if (!rule) throw new Error(`no style rule for "${selector}"`);
  return rule.style;
}

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

describe("turning the model into elements", () => {
  it("emits a node per node and an edge per edge", () => {
    const elements = toElements(picture());

    expect(elements.filter((e) => e.group === "nodes")).toHaveLength(2);
    expect(elements.filter((e) => e.group === "edges")).toHaveLength(1);
  });

  /** Cytoscape rejects an edge whose endpoints it has not seen, so an edge
   *  listed first is dropped rather than deferred. */
  it("lists every node before any edge", () => {
    const elements = toElements(picture());
    const groups = elements.map((e) => e.group);
    const lastNode = groups.lastIndexOf("nodes");
    const firstEdge = groups.indexOf("edges");

    expect(lastNode).toBeLessThan(firstEdge);
  });

  /** An edge to a node that is not in the picture happens legitimately — a
   *  diff can hold one whose far end authorization filtered out — and
   *  Cytoscape throws on it rather than skipping it. */
  it("drops an edge whose endpoint is not in the picture", () => {
    const elements = toElements(
      picture({ edges: [{ from: "a", to: "ghost", relationship: "feeds" }] }),
    );

    expect(elements.filter((e) => e.group === "edges")).toHaveLength(0);
    expect(elements.filter((e) => e.group === "nodes")).toHaveLength(2);
  });

  /** And the negative: a *present* endpoint must not be dropped, or the filter
   *  above would be satisfied by drawing no edges at all. */
  it("keeps an edge whose endpoints are both present", () => {
    expect(toElements(picture()).filter((e) => e.group === "edges")).toHaveLength(1);
  });

  it("carries the node name as the label a reader sees", () => {
    const node = toElements(picture()).find((e) => e.data.id === "a");

    expect(node?.data.label).toBe("upi_transactions");
  });

  /** Cytoscape draws this straight onto a `<canvas>`, which has no `dir`
   *  attribute — a right-to-left name mixed with left-to-right text has to
   *  arrive with its runs already in the position `fillText` needs to draw
   *  them correctly (`bidiLabel.ts`'s own doc comment; empirically verified
   *  against a real browser, not merely against `bidi-js`'s output). Real
   *  Hebrew text ("customer" — a stand-in fixture, not asserted for its
   *  meaning), not a placeholder, matching this project's own standing rule
   *  for bidi RED tests. A *standalone* right-to-left name is a weaker
   *  fixture here — `fillText` reverses that correctly on its own, so
   *  `canvasLabel` leaves it untouched and passing that through this test
   *  would not distinguish "shaped" from "never called". Mutator watch:
   *  dropping the `canvasLabel` call must fail this. */
  it("repositions a right-to-left node name's runs for canvas rendering rather than passing it through unchanged", () => {
    const rtlName = "לקוח_orders";
    const graph = picture({ nodes: [{ id: "a", name: rtlName, kind: "table" }] });

    const node = toElements(graph).find((e) => e.data.id === "a");

    expect(node?.data.label).not.toBe(rtlName);
    expect(node?.data.label).toBe("orders_לקוח");
  });
});

describe("edge identity", () => {
  /** `a contains b` and `a feeds b` are two facts about one pair. An id built
   *  from the endpoints alone makes Cytoscape silently drop the second — an
   *  edge vanishing because of how it was keyed, not because of the graph. */
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

  it("keeps both edges of a pair in the elements", () => {
    const elements = toElements(
      picture({
        edges: [
          { from: "a", to: "b", relationship: "contains" },
          { from: "a", to: "b", relationship: "feeds" },
        ],
      }),
    );

    const ids = elements.filter((e) => e.group === "edges").map((e) => e.data.id);
    expect(new Set(ids).size).toBe(2);
  });
});

/** Asserted as a *split list*, never with `toContain`. Joining the classes
 *  without a separator yields one garbage class name that matches every
 *  substring test while breaking every style rule — a failure that is invisible
 *  to an assertion phrased as "contains". */
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

  /** And the negative: every node carrying `seed` makes the class meaningless
   *  and draws the whole neighbourhood at seed size. */
  it("marks only the seed", () => {
    const p = picture();
    expect(classesOf(p.nodes[1]!, p)).not.toContain("seed");
  });

  it("marks an unexpanded node as expandable, and an expanded one not", () => {
    const p = picture();
    expect(classesOf(p.nodes[1]!, p)).toContain("expandable");
    expect(classesOf(p.nodes[0]!, p)).not.toContain("expandable");
  });

  /** The marker sits on the node that is hiding something, not on the canvas —
   *  so a reader can tell *where* the picture is incomplete. */
  it("marks the node that is hiding neighbours", () => {
    const p = picture({ truncatedAt: ["b"] });

    expect(classesOf(p.nodes[1]!, p)).toContain("truncated");
    expect(classesOf(p.nodes[0]!, p)).not.toContain("truncated");
  });

  /** `op = false` is a retraction: a node present at the earlier instant and
   *  absent at the later one is still drawn, marked, so the picture shows a
   *  deletion rather than an absence. */
  it("carries the diff change so a removed node can still be drawn", () => {
    const p = picture({
      nodes: [{ id: "a", name: "gone", kind: "table", change: "removed" }],
    });

    expect(classesOf(p.nodes[0]!, p)).toContain("removed");
  });

  it("defaults to unchanged when there is no comparison", () => {
    expect(classesOf(picture().nodes[0]!, picture())).toContain("unchanged");
  });

  /** A node the reader may not see keeps its place but not its kind — removing
   *  it would claim a smaller neighbourhood than exists. */
  it("marks a node whose kind is hidden by authorization", () => {
    const p = picture({ nodes: [{ id: "a", name: "?", kind: null }] });

    expect(classesOf(p.nodes[0]!, p)).toContain("hidden-kind");
  });

  it("does not mark an ordinary node as hidden", () => {
    expect(classesOf(picture().nodes[1]!, picture())).not.toContain("hidden-kind");
  });
});

describe("the layout is deterministic", () => {
  /** A force simulation settles somewhere slightly different every run, so the
   *  same neighbourhood never looks the same twice and nobody can point at
   *  "the node on the left". */
  it("is breadthfirst, rooted at the seed, and never animated", () => {
    const options = layoutOptions("a");

    expect(options.name).toBe("breadthfirst");
    expect(options.roots).toEqual(["a"]);
    expect(options.animate).toBe(false);
  });

  it("produces identical options for the same seed", () => {
    expect(layoutOptions("a")).toEqual(layoutOptions("a"));
  });

  /** Every option asserted, because each one is a determinism decision and
   *  none of them is visible in the result of a unit test otherwise.
   *  `directed: false` so the rings reflect *reachability* rather than edge
   *  direction — a column pointing at its table would otherwise sit a ring
   *  further out than the same column reached the other way. `maximal` and
   *  `grid` are what make sibling order stable instead of insertion-ordered. */
  it("pins every option the layout's determinism rests on", () => {
    expect(layoutOptions("a")).toEqual({
      name: "breadthfirst",
      roots: ["a"],
      directed: false,
      animate: false,
      maximal: true,
      grid: true,
      spacingFactor: 1.1,
      padding: 24,
    });
  });

  it("roots at whichever node the canvas opened on", () => {
    expect(layoutOptions("z").roots).toEqual(["z"]);
  });
});

describe("choosing a renderer", () => {
  /** WebGL has a fixed context-creation cost and a texture atlas that only pays
   *  for itself once there is enough to draw, so switching it on for a six-node
   *  neighbourhood makes the common case slower to open. */
  it("stays on canvas for a small neighbourhood", () => {
    expect(wantsWebgl(0)).toBe(false);
    expect(wantsWebgl(6)).toBe(false);
    expect(wantsWebgl(WEBGL_THRESHOLD - 1)).toBe(false);
  });

  it("switches on at the threshold and above", () => {
    expect(wantsWebgl(WEBGL_THRESHOLD)).toBe(true);
    expect(wantsWebgl(10_000)).toBe(true);
  });

  /** The threshold must stay an order of magnitude below `00f`'s 10,000-node
   *  interactivity budget, so the budget is never the thing being tested. */
  it("sits well below the interactivity budget", () => {
    expect(WEBGL_THRESHOLD).toBeLessThan(1_000);
  });
});

describe("a derived edge is drawn differently", () => {
  // **Not decoration.** `00b` decision 2 keeps conclusions in their own graph
  // precisely so nobody mistakes one for something a person asserted, and a
  // picture that renders both alike undoes that separation in front of the
  // reader most likely to act on it.
  it("carries a class an asserted edge does not", () => {
    const derived = edgeClasses({ from: "a", to: "b", relationship: "feeds", derived: true });
    const asserted = edgeClasses({ from: "a", to: "b", relationship: "feeds", derived: false });

    expect(derived.split(" ")).toContain("derived");
    expect(asserted.split(" ")).not.toContain("derived");
  });

  // An older server that does not send the flag understates what the reasoner
  // did rather than overstating it — the safe direction for a claim about
  // provenance.
  it("treats an absent flag as asserted", () => {
    expect(edgeClasses({ from: "a", to: "b", relationship: "feeds" }).split(" ")).not.toContain(
      "derived",
    );
  });

  // The diff classes still apply: an edge can be both newly added and derived,
  // and losing either would misreport what changed or where it came from.
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

  it("reaches the elements the canvas draws", () => {
    const elements = toElements({
      seedId: "a",
      nodes: [
        { id: "a", name: "a", kind: "table" },
        { id: "b", name: "b", kind: "table" },
      ],
      edges: [{ from: "a", to: "b", relationship: "feeds", derived: true }],
      expanded: ["a", "b"],
      truncatedAt: [],
    });

    const edge = elements.find((e) => e.group === "edges");
    expect(edge?.classes.split(" ")).toContain("derived");
  });
});

describe("the style a reader can actually see", () => {
  // Plan 114 Slice A: `data.label` was already attached to every edge — the
  // gap was that no style rule ever read it back. A reader relying on the
  // canvas alone (rather than a separate text panel) could not tell one
  // relationship from another.
  it("draws the relationship name on every edge, not only the node name", () => {
    expect(ruleFor("edge").label).toBe("data(label)");
    expect(ruleFor("node").label).toBe("data(label)");
  });

  // `source`/`target` are a directed fact; without an arrow the canvas draws
  // an undirected line and throws that fact away.
  it("draws a directed arrow on every edge", () => {
    expect(ruleFor("edge")["target-arrow-shape"]).toBe("triangle");
  });

  // A derived edge already draws its line in the brand cyan so it reads as a
  // conclusion rather than an assertion (`00b` decision 2) — the arrowhead
  // has to match, or the one directed cue on a derived edge is drawn in the
  // wrong colour.
  it("matches a derived edge's arrow to its own line colour, not the default", () => {
    const base = ruleFor("edge");
    const derived = ruleFor("edge.derived");

    expect(derived["target-arrow-color"]).toBe(derived["line-color"]);
    expect(derived["target-arrow-color"]).not.toBe(base["target-arrow-color"]);
  });

  // An edge label sitting directly on top of a crossing line is unreadable —
  // it needs a background so it reads as a label rather than noise.
  it("gives the edge label a background so it survives a crossing line", () => {
    const edge = ruleFor("edge");
    expect(edge["text-background-opacity"]).toBeGreaterThan(0);
    expect(edge["text-background-color"]).toBe(colors.raised);
  });

  // A curved edge is Cytoscape's default for a pair with more than one edge
  // between them; straight keeps a single relationship reading as one
  // direct line rather than an arbitrary bow.
  it("keeps the edge straight rather than an arbitrary curve", () => {
    expect(ruleFor("edge")["curve-style"]).toBe("straight");
  });

  it("places the node label below the node, not on top of it", () => {
    expect(ruleFor("node")["text-valign"]).toBe("bottom");
  });

  // Verified live: fitting a real 2-node evidence graph with no cap zoomed
  // far enough to render an 18px node as a ~100px circle. The cap has to be
  // low enough that this stays legible and high enough that `.fit()` still
  // does real work on a spread-out neighbourhood.
  it("caps how far a sparse graph can be zoomed to fill the canvas", () => {
    expect(MAX_ZOOM).toBe(2);
  });
});

// Every rule this function carried over unchanged from the inline array in
// `GraphCanvas.tsx` — full-object equality per rule, so a mutant that empties
// or drops one property is caught the same way as one that drops the whole
// rule, without a separate assertion per property.
describe("every class a reader can act on keeps its own drawing rule", () => {
  it("the seed stands out by size and weight", () => {
    expect(ruleFor("node.seed")).toEqual({ width: 26, height: 26, "font-weight": "bold" });
  });

  it("an expandable node is marked by a ring, not a colour", () => {
    expect(ruleFor("node.expandable")).toEqual({
      "border-width": 3,
      "border-color": colors.primary,
      "background-opacity": 0.35,
    });
  });

  it("a node hiding neighbours is marked by a dashed border", () => {
    expect(ruleFor("node.truncated")).toEqual({
      "border-width": 3,
      "border-style": "dashed",
      "border-color": colors.text,
    });
  });

  it("a node whose kind is hidden by authorization loses its colour", () => {
    expect(ruleFor("node.hidden-kind")).toEqual({ "background-color": colors.border });
  });

  it("a removed node is marked by shape and opacity, not colour alone", () => {
    expect(ruleFor("node.removed")).toEqual({
      shape: "diamond",
      "background-opacity": 0.4,
      "border-style": "dashed",
      "border-width": 2,
      "border-color": colors.text,
    });
  });

  it("an added node is marked by shape alone", () => {
    expect(ruleFor("node.added")).toEqual({ shape: "star" });
  });

  it("a derived edge is dashed and tinted, arrow included", () => {
    expect(ruleFor("edge.derived")).toEqual({
      "line-style": "dashed",
      "line-color": brand.cyan400,
      "target-arrow-color": brand.cyan400,
    });
  });

  it("a removed edge is dashed in the text colour, not the line colour", () => {
    expect(ruleFor("edge.removed")).toEqual({ "line-style": "dashed", "line-color": colors.text });
  });

  it("an added edge is thicker and in the primary colour", () => {
    expect(ruleFor("edge.added")).toEqual({ width: 2, "line-color": colors.primary });
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

  it("removes a hidden node and drops its incident edges from what reaches the elements", () => {
    const elements = toElements(visiblePicture(threeNode, new Set(["b"])));

    expect(elements.some((e) => e.data.id === "b")).toBe(false);
    expect(elements.some((e) => e.group === "edges" && e.data.id === edgeId(aContainsB))).toBe(false);
    // The other node and its own edge are untouched by hiding a different one.
    expect(elements.some((e) => e.data.id === "c")).toBe(true);
    expect(elements.some((e) => e.group === "edges" && e.data.id === edgeId(aContainsC))).toBe(true);
  });

  it("hiding a node with no edges of its own leaves the rest of the picture unchanged", () => {
    const isolated: Picture = {
      ...threeNode,
      nodes: [...threeNode.nodes, { id: "d", name: "d", kind: "column" }],
    };

    const elements = toElements(visiblePicture(isolated, new Set(["d"])));

    expect(elements).toEqual(toElements(threeNode));
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

  it("falls back to the border colour for a hidden kind, matching node.hidden-kind", () => {
    expect(kindColor(null, "light", colors)).toBe(colors.border);
  });

  it("uses a different set of hexes for dark mode, not the same colours regardless of theme", () => {
    for (const kind of KINDS) {
      expect(kindColor(kind, "dark", colors)).not.toBe(kindColor(kind, "light", colors));
    }
  });

  // The exact hexes, not just "5 distinct values" — these are the ones that
  // ran through the dataviz skill's validator against this project's own
  // surfaces; a silently-changed value would no longer be the validated one.
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

  it("the base node style reads its fill from data, not a fixed colour", () => {
    expect(ruleFor("node")["background-color"]).toBe("data(color)");
  });

  it("carries a kind's colour into the element data toElements builds", () => {
    const elements = toElements(
      { seedId: "a", nodes: [{ id: "a", name: "a", kind: "table" }], edges: [], expanded: ["a"], truncatedAt: [] },
      "light",
    );
    const node = elements.find((e) => e.data.id === "a");
    expect(node?.data.color).toBe(kindColor("table", "light", colors));
  });

  it("leaves a hidden-kind node's data without a colour, so only node.hidden-kind's style decides it", () => {
    const elements = toElements({
      seedId: "a",
      nodes: [{ id: "a", name: "a", kind: null }],
      edges: [],
      expanded: ["a"],
      truncatedAt: [],
    });
    const data = elements.find((e) => e.data.id === "a")?.data;
    // Not `.toBeUndefined()` on the value alone: an *absent* key and a key
    // present with value `undefined` both read that way, and only the first
    // is the real contract — a lookup with a `null` kind must never run at
    // all, not run and happen to land on `undefined`.
    expect(data && "color" in data).toBe(false);
  });
});

describe("the legend — Plan 114 Slice D", () => {
  it("names every kind toElements can colour, in the same fixed order", () => {
    const entries = legendEntries("light", colors);
    expect(entries.map((e) => e.kind)).toEqual(["service", "database", "schema", "table", "column"]);
  });

  it("each entry's colour is exactly what a node of that kind would be drawn in", () => {
    for (const entry of legendEntries("dark", colors)) {
      expect(entry.color).toBe(kindColor(entry.kind, "dark", colors));
    }
  });

  it("labels read for a human, not the raw kind string", () => {
    const table = legendEntries("light", colors).find((e) => e.kind === "table");
    expect(table?.label).toBe("Table");
  });
});
