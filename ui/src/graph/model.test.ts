import { describe, expect, it } from "vitest";
import { type GraphModel, expand, seed } from "./model";
import type { GraphView } from "../api";

/** Factories, not fixtures: every test gets its own graph, so one test cannot
 *  leave state behind that another depends on. */
function view(overrides?: Partial<GraphView>): GraphView {
  return {
    nodes: [
      { id: "a", name: "upi_transactions", kind: "table" },
      { id: "b", name: "amount", kind: "column" },
    ],
    edges: [{ from: "a", to: "b", relationship: "contains" }],
    truncated: false,
    ...overrides,
  };
}

function ids(model: GraphModel): string[] {
  return model.nodes.map((n) => n.id).sort();
}

describe("seeding", () => {
  it("takes the nodes and edges of the walk it was given", () => {
    const model = seed("a", view());
    expect(ids(model)).toEqual(["a", "b"]);
    expect(model.edges).toHaveLength(1);
    expect(model.seedId).toBe("a");
  });

  it("marks the seed as expanded, so it is not offered as expandable again", () => {
    expect(seed("a", view()).expanded).toEqual(["a"]);
  });
});

describe("expansion", () => {
  it("adds the nodes and edges the expansion returned", () => {
    const model = expand(seed("a", view()), "b", {
      nodes: [
        { id: "b", name: "amount", kind: "column" },
        { id: "c", name: "audit_log", kind: "table" },
      ],
      edges: [{ from: "c", to: "b", relationship: "feeds" }],
      truncated: false,
    });

    expect(ids(model)).toEqual(["a", "b", "c"]);
    expect(model.edges).toHaveLength(2);
  });

  /** **The double-expand test.** A duplicated node makes degree counts wrong,
   *  and degree is what Epic 38 surfaces as blast radius — so a node counted
   *  twice is a blast radius reported at twice its size. */
  it("expanding the same node twice adds nothing the second time", () => {
    const once = expand(seed("a", view()), "b", {
      nodes: [
        { id: "b", name: "amount", kind: "column" },
        { id: "c", name: "audit_log", kind: "table" },
      ],
      edges: [{ from: "c", to: "b", relationship: "feeds" }],
      truncated: false,
    });
    const twice = expand(once, "b", {
      nodes: [
        { id: "b", name: "amount", kind: "column" },
        { id: "c", name: "audit_log", kind: "table" },
      ],
      edges: [{ from: "c", to: "b", relationship: "feeds" }],
      truncated: false,
    });

    expect(ids(twice)).toEqual(["a", "b", "c"]);
    expect(twice.edges).toHaveLength(2);
    // And the record of what has been explored survives the repeat: losing it
    // would offer the node for expansion again, forever.
    expect([...twice.expanded].sort()).toEqual(["a", "b"]);
  });

  /** Identity is the node's id, never its position in the array — the same
   *  entity reached by two different walks is one node, or selection, deep
   *  links and diff all silently break. */
  it("a node returned by two walks stays one node", () => {
    const model = expand(seed("a", view()), "b", {
      nodes: [{ id: "a", name: "upi_transactions", kind: "table" }],
      edges: [{ from: "a", to: "b", relationship: "contains" }],
      truncated: false,
    });
    expect(ids(model)).toEqual(["a", "b"]);
  });

  /** An edge is identified by both endpoints *and* its relationship: `a
   *  contains b` and `a feeds b` are two facts about the same pair, and
   *  collapsing them would hide one. */
  it("two relationships between the same pair are two edges", () => {
    const model = expand(seed("a", view()), "b", {
      nodes: [],
      edges: [{ from: "a", to: "b", relationship: "feeds" }],
      truncated: false,
    });
    expect(model.edges).toHaveLength(2);
  });

  it("records what has been expanded, so the same node is not offered twice", () => {
    const model = expand(seed("a", view()), "b", view());
    expect([...model.expanded].sort()).toEqual(["a", "b"]);
  });
});

describe("truncation", () => {
  /** **The silent-truncation test.** This is the most damaging bug this screen
   *  can have: the user concludes "nothing else depends on this" from an
   *  absence the system created. Once any part of the picture is truncated,
   *  the picture stays truncated — a later complete expansion does not make
   *  the earlier omission go away. */
  it("a truncated expansion leaves the model truncated", () => {
    const model = expand(seed("a", view()), "b", {
      nodes: [],
      edges: [],
      truncated: true,
    });
    expect(model.truncated).toBe(true);
  });

  it("a complete expansion does not clear an earlier truncation", () => {
    const truncated = expand(seed("a", view({ truncated: true })), "b", {
      nodes: [],
      edges: [],
      truncated: false,
    });
    expect(truncated.truncated).toBe(true);
  });

  it("names which nodes were truncated, so the marker can be placed", () => {
    const model = expand(seed("a", view()), "b", {
      nodes: [],
      edges: [],
      truncated: true,
    });
    expect(model.truncatedAt).toContain("b");
  });

  /** A truncated *seed* hides something too. Marking only truncated
   *  expansions would leave the opening view silently incomplete, which is
   *  the state a reader is least likely to question. */
  it("a truncated seed names the seed as hiding more", () => {
    expect(seed("a", view({ truncated: true })).truncatedAt).toEqual(["a"]);
  });

  /** The marker belongs to the node, and a later complete expansion elsewhere
   *  says nothing about the node that was truncated earlier. */
  it("a later complete expansion does not clear an earlier node's marker", () => {
    const truncated = expand(seed("a", view()), "b", {
      nodes: [],
      edges: [],
      truncated: true,
    });
    const then = expand(truncated, "c", view());

    expect(then.truncatedAt).toContain("b");
  });

  it("an untruncated graph is not marked", () => {
    const model = expand(seed("a", view()), "b", view());
    expect(model.truncated).toBe(false);
    expect(model.truncatedAt).toEqual([]);
  });
});

describe("immutability", () => {
  /** React re-renders on identity change. An expansion that mutated the
   *  previous model in place would update the data and not the picture. */
  it("expansion returns a new model and leaves the previous one alone", () => {
    const before = seed("a", view());
    const after = expand(before, "b", {
      nodes: [{ id: "c", name: "audit_log", kind: "table" }],
      edges: [{ from: "c", to: "b", relationship: "feeds" }],
      truncated: false,
    });

    expect(after).not.toBe(before);
    expect(ids(before)).toEqual(["a", "b"]);
  });
});
