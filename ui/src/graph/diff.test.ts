import { describe, expect, it } from "vitest";
import { diff } from "./diff";
import { seed } from "./model";
import type { GraphView } from "../api";

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

describe("diffing two instants", () => {
  it("an unchanged graph reports everything as unchanged", () => {
    const result = diff(seed("a", view()), seed("a", view()));
    expect(result.nodes.map((n) => n.change)).toEqual(["unchanged", "unchanged"]);
    expect(result.edges.map((e) => e.change)).toEqual(["unchanged"]);
  });

  /** **The retraction test, from the model's side.** An entity deleted last
   *  week must appear when viewing last month — so a node present at `before`
   *  and gone at `after` is *removed*, and it stays in the result. Dropping it
   *  would render the past and the present identically, which is the whole
   *  differentiator quietly failing. */
  it("a node present before and absent after is removed, and still listed", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({
        nodes: [{ id: "a", name: "upi_transactions", kind: "table" }],
        edges: [],
      }),
    );

    const result = diff(before, after);
    const b = result.nodes.find((n) => n.id === "b");
    expect(b?.change).toBe("removed");
    expect(b?.name).toBe("amount");
  });

  it("a node absent before and present after is added", () => {
    const before = seed(
      "a",
      view({ nodes: [{ id: "a", name: "upi_transactions", kind: "table" }], edges: [] }),
    );
    const result = diff(before, seed("a", view()));

    expect(result.nodes.find((n) => n.id === "b")?.change).toBe("added");
  });

  /** A node can survive an interval and still not be the same: a rename is a
   *  change someone needs to see, and reporting it as unchanged because the id
   *  matched is how a diff view becomes untrustworthy. */
  it("a node whose name changed is changed, not unchanged", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({
        nodes: [
          { id: "a", name: "upi_transactions", kind: "table" },
          { id: "b", name: "txn_amount", kind: "column" },
        ],
      }),
    );

    const result = diff(before, after);
    const b = result.nodes.find((n) => n.id === "b");
    expect(b?.change).toBe("changed");
    // The *later* name is what the reader is shown, with the earlier one
    // available beside it — a diff that showed only the old name would name
    // something that no longer exists.
    expect(b?.name).toBe("txn_amount");
    expect(b?.wasName).toBe("amount");
  });

  it("a node whose kind changed is changed", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({
        nodes: [
          { id: "a", name: "upi_transactions", kind: "table" },
          { id: "b", name: "amount", kind: "table" },
        ],
      }),
    );
    const b = diff(before, after).nodes.find((n) => n.id === "b");
    expect(b?.change).toBe("changed");
    // A kind change is not a rename. Reporting a `wasName` identical to the
    // current one would render "amount → amount" and teach the reader that
    // the diff view invents changes.
    expect(b?.wasName).toBeUndefined();
  });

  it("an unchanged node carries no former name", () => {
    const result = diff(seed("a", view()), seed("a", view()));
    expect(result.nodes.every((n) => n.wasName === undefined)).toBe(true);
  });

  it("an edge present only after is added, and only before is removed", () => {
    const before = seed("a", view({ edges: [] }));
    const after = seed("a", view());

    expect(diff(before, after).edges[0]?.change).toBe("added");
    expect(diff(after, before).edges[0]?.change).toBe("removed");
  });

  /** The relationship is part of an edge's identity, so re-typing an edge is
   *  one removal and one addition — not an unchanged edge. */
  it("an edge whose relationship changed is a removal and an addition", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({ edges: [{ from: "a", to: "b", relationship: "feeds" }] }),
    );

    const changes = diff(before, after)
      .edges.map((e) => e.change)
      .sort();
    expect(changes).toEqual(["added", "removed"]);
  });

  it("counts each kind of change, so the summary cannot disagree with the picture", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({
        nodes: [
          { id: "a", name: "upi_transactions", kind: "table" },
          { id: "c", name: "audit_log", kind: "table" },
        ],
        edges: [{ from: "a", to: "c", relationship: "feeds" }],
      }),
    );

    const result = diff(before, after);
    // Node `b` went and `c` arrived; the edge to `b` went with it and a new
    // one arrived with `c`. The summary counts nodes and edges together — the
    // reader is told how much moved, not how much of each shape moved.
    expect(result.summary).toEqual({ added: 2, removed: 2, changed: 0 });
    expect(result.nodes.filter((n) => n.change === "added")).toHaveLength(1);
    expect(result.edges.filter((e) => e.change === "added")).toHaveLength(1);
  });

  it("counts a rename as changed rather than as an arrival and a departure", () => {
    const before = seed("a", view());
    const after = seed(
      "a",
      view({
        nodes: [
          { id: "a", name: "upi_transactions", kind: "table" },
          { id: "b", name: "txn_amount", kind: "column" },
        ],
      }),
    );

    expect(diff(before, after).summary).toEqual({ added: 0, removed: 0, changed: 1 });
  });

  /** Truncation on either side taints the comparison: a node "missing" at one
   *  instant may simply not have been fetched, and a diff that presented that
   *  as a removal would invent a deletion that never happened. */
  it("a truncated side makes the whole comparison partial", () => {
    const before = seed("a", view({ truncated: true }));
    expect(diff(before, seed("a", view())).partial).toBe(true);
    expect(diff(seed("a", view()), before).partial).toBe(true);
  });

  it("two complete sides compare completely", () => {
    expect(diff(seed("a", view()), seed("a", view())).partial).toBe(false);
  });
});
