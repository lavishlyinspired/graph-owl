import { describe, expect, it } from "vitest";
import { connectivityRows, describeAnalytics } from "./analytics";
import type { AssetAnalytics } from "../api";

function analytics(overrides?: Partial<AssetAnalytics>): AssetAnalytics {
  return {
    nodes: ["1:a", "1:b", "1:c"],
    inDegree: [2, 0, 1],
    outDegree: [1, 0, 0],
    orphans: ["1:b"],
    cycles: [],
    edgeTypes: ["1:owns"],
    truncated: false,
    ...overrides,
  };
}

describe("turning raw analytics into rows", () => {
  it("pairs each node with its own in/out degree by position", () => {
    const rows = connectivityRows(analytics());
    const a = rows.find((r) => r.id === "1:a");
    expect(a).toMatchObject({ inDegree: 2, outDegree: 1 });
  });

  it("orders the most connected node first", () => {
    const rows = connectivityRows(analytics());
    expect(rows[0]?.id).toBe("1:a");
  });

  it("flags a node named in `orphans` as an orphan", () => {
    const rows = connectivityRows(analytics());
    expect(rows.find((r) => r.id === "1:b")?.orphan).toBe(true);
  });

  it("does not flag a connected node as an orphan", () => {
    const rows = connectivityRows(analytics());
    expect(rows.find((r) => r.id === "1:a")?.orphan).toBe(false);
  });

  it("prefers a caller-supplied name over the bare id", () => {
    const rows = connectivityRows(analytics(), new Map([["1:a", "Invoice 42"]]));
    expect(rows.find((r) => r.id === "1:a")?.label).toBe("Invoice 42");
  });

  it("falls back to the id's own local part when no name is supplied", () => {
    const rows = connectivityRows(analytics());
    expect(rows.find((r) => r.id === "1:c")?.label).toBe("c");
  });

  it("refuses to pair mismatched vectors rather than silently misattributing them", () => {
    expect(() => connectivityRows(analytics({ inDegree: [1] }))).toThrow();
  });
});

describe("describing what the numbers cover", () => {
  it("states the node count and edge kinds for a complete walk", () => {
    expect(describeAnalytics(analytics())).toBe("3 nodes in this neighbourhood, connected by owns.");
  });

  it("says so when nothing connects the nodes", () => {
    expect(describeAnalytics(analytics({ edgeTypes: [] }))).toBe(
      "3 nodes in this neighbourhood, no relationships between them.",
    );
  });

  it("states truncation explicitly rather than letting the count imply completeness", () => {
    expect(describeAnalytics(analytics({ truncated: true }))).toBe(
      "3 nodes reached before the walk stopped at its limit, connected by owns.",
    );
  });
});
