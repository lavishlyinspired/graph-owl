import { describe, expect, it } from "vitest";
import {
  edgeClasses,
  type Picture,
  WEBGL_THRESHOLD,
  edgeId,
  layoutOptions,
  nodeClasses,
  toElements,
  wantsWebgl,
} from "./cytoscape";

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
