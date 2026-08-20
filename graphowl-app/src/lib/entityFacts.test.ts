import { describe, expect, it } from "vitest";
import { factsFromEdges, impactFromEdges } from "./entityFacts";
import type { GraphView } from "./api";

function view(overrides?: Partial<GraphView>): GraphView {
  return {
    nodes: [
      { id: "a", name: "Patel Chemicals & Co", kind: null },
      { id: "b", name: "books-INV-006", kind: null },
    ],
    edges: [{ from: "a", to: "b", relationship: "issuedBy" }],
    truncated: false,
    ...overrides,
  };
}

describe("an entity's outgoing facts", () => {
  it("names the target by its resolved label, not its raw id", () => {
    expect(factsFromEdges(view())).toEqual([
      { relationship: "issuedBy", target: "books-INV-006", derived: false },
    ]);
  });

  it("falls back to the bare id when the target has no node in this picture", () => {
    const facts = factsFromEdges(
      view({ edges: [{ from: "a", to: "ghost", relationship: "feeds" }] }),
    );
    expect(facts[0]?.target).toBe("ghost");
  });

  it("carries whether the reasoner concluded the fact, not the reader", () => {
    const facts = factsFromEdges(
      view({ edges: [{ from: "a", to: "b", relationship: "locatedIn", derived: true }] }),
    );
    expect(facts[0]?.derived).toBe(true);
  });

  it("reads asserted when derived is absent, understating rather than overstating", () => {
    const facts = factsFromEdges(view({ edges: [{ from: "a", to: "b", relationship: "issuedBy" }] }));
    expect(facts[0]?.derived).toBe(false);
  });

  it("lists nothing for a subject with no outgoing edges", () => {
    expect(factsFromEdges(view({ edges: [] }))).toEqual([]);
  });
});

describe("an entity's impact — what points at it, grouped by relationship", () => {
  it("counts how many incoming edges carry each relationship", () => {
    const impact = impactFromEdges([
      { from: "x", to: "a", relationship: "onInvoice" },
      { from: "y", to: "a", relationship: "onInvoice" },
      { from: "z", to: "a", relationship: "recordedIn" },
    ]);
    expect(impact).toEqual([
      { label: "onInvoice", n: 2 },
      { label: "recordedIn", n: 1 },
    ]);
  });

  it("reports nothing for a subject nothing else in the graph points at", () => {
    expect(impactFromEdges([])).toEqual([]);
  });
});
