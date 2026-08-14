import { describe, expect, it } from "vitest";
import { edgeRows, nodeRows, summarize } from "./subjectContext";
import type { GraphContext } from "../api";

const context = (overrides?: Partial<GraphContext>): GraphContext => ({
  nodes: [
    { id: "1024:invoice-1", iri: "https://graph-owl.dev/packs/gst#invoice-1", sources: ["gst-purchase-register"] },
    { id: "1024:supplier-1", iri: "https://graph-owl.dev/packs/gst#supplier-1", sources: ["gst-gstr2b", "gst-purchase-register"] },
  ],
  edges: [{ from: "1024:invoice-1", to: "1024:supplier-1", relationship: "issuedBy", derived: false }],
  truncated: false,
  ...overrides,
});

describe("rendering a subject's neighbourhood as a list", () => {
  /** **`00f`'s own non-negotiable**: a graph picture needs a list-equivalent a
   *  screen reader and a keyboard can reach, not only a canvas. This module
   *  is that half, built first — the same posture `GraphExplorer`'s own
   *  `<details>` section already takes for catalog assets, generalized to a
   *  subject with none. */
  it("names each node and its sources, with a known name preferred over the bare id", () => {
    const rows = nodeRows(context(), new Map([["1024:invoice-1", "INV-1001"]]));
    expect(rows).toEqual([
      { id: "1024:invoice-1", label: "INV-1001", sources: ["gst-purchase-register"] },
      { id: "1024:supplier-1", label: "1024:supplier-1", sources: ["gst-gstr2b", "gst-purchase-register"] },
    ]);
  });

  it("lists each edge as subject/relationship/object, resolved through the same names", () => {
    const rows = edgeRows(context(), new Map([["1024:invoice-1", "INV-1001"]]));
    expect(rows).toEqual([
      { from: "INV-1001", relationship: "issuedBy", to: "1024:supplier-1", derived: false },
    ]);
  });

  /** A node this console never asserted a name for reads as its identity —
   *  the same "never invent" rule `paths.ts`'s `nodeLabel` already commits to,
   *  applied here through the same function rather than a second copy of it. */
  it("falls back to the bare id rather than inventing a name", () => {
    expect(nodeRows(context(), new Map())[0]!.label).toBe("1024:invoice-1");
  });
});

describe("stating what was found", () => {
  it("counts nodes and edges, and states truncation rather than hiding it", () => {
    expect(summarize(context())).toBe("2 nodes, 1 relationship.");
    expect(summarize(context({ truncated: true }))).toContain("stopped");
  });

  it("says so plainly when the seed has no neighbourhood at all", () => {
    const empty = context({ nodes: [context().nodes[0]!], edges: [] });
    expect(summarize(empty).toLowerCase()).toContain("nothing connected");
  });

  /** **Both halves of the guard matter, checked independently.** A subject
   *  with more than one node reached (real neighbours) but zero edges
   *  reported would otherwise fall into the "nothing connected" branch on the
   *  `||` mutant, understating a neighbourhood that is genuinely there. */
  it("does not say nothing connected when nodes were reached but edges were not counted yet", () => {
    const text = summarize(
      context({ nodes: [context().nodes[0]!, context().nodes[1]!], edges: [] }),
    );
    expect(text.toLowerCase()).not.toContain("nothing connected");
    expect(text).toBe("2 nodes, 0 relationships.");
  });

  /** And the inverse: exactly one node but a real edge (a self-loop, or a
   *  node whose only neighbour is itself) must not read as empty either. */
  it("does not say nothing connected when an edge exists even with a single node", () => {
    const single = context({ nodes: [context().nodes[0]!] });
    expect(summarize(single).toLowerCase()).not.toContain("nothing connected");
  });

  /** Singular vs plural, both directions — a mutant that hardcoded the
   *  plural form passed every fixture above, which happens to have two of
   *  everything. */
  it("uses the singular for exactly one node or one relationship", () => {
    expect(summarize(context({ nodes: [context().nodes[0]!], edges: [context().edges[0]!] }))).toBe(
      "1 node, 1 relationship.",
    );
  });
});
