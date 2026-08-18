import { describe, expect, it } from "vitest";
import { connectivityRows, describeAnalytics } from "./analytics";
import type { AssetAnalytics } from "../api";

const analytics = (overrides?: Partial<AssetAnalytics>): AssetAnalytics => ({
  nodes: ["1:a", "1:b", "1:c"],
  inDegree: [0, 2, 0],
  outDegree: [2, 0, 0],
  orphans: ["1:c"],
  edgeTypes: ["contains"],
  truncated: false,
  ...overrides,
});

describe("reading a neighbourhood's connectivity", () => {
  /** **The three vectors are index-aligned and the server says so.** Joining
   *  them by position is the contract; a row built by reordering either side
   *  would attribute one node's connectivity to another, which is a wrong
   *  answer that looks like a working table. */
  /** **Found in a browser: every row was a raw UUID.** An asset's graph
   *  identity is a UUID, and the canvas above resolves names for exactly these
   *  nodes — so a table that shows only ids makes the reader match hex strings
   *  by eye. Prefer a known name, fall back to the identifier, never invent
   *  one: the same rule `paths.ts` already applies. */
  it("prefers a name the caller already knows for the node", () => {
    const rows = connectivityRows(analytics(), new Map([["1:a", "warehouse.public.orders"]]));
    expect(rows.find((r) => r.id === "1:a")!.label).toBe("warehouse.public.orders");
    // Unknown ids still read as their identity rather than as a blank.
    expect(rows.find((r) => r.id === "1:b")!.label).toBe("b");
  });

  it("joins the degree vectors by position", () => {
    const rows = connectivityRows(analytics());
    expect(rows).toEqual([
      { id: "1:a", label: "a", inDegree: 0, outDegree: 2, orphan: false },
      { id: "1:b", label: "b", inDegree: 2, outDegree: 0, orphan: false },
      { id: "1:c", label: "c", inDegree: 0, outDegree: 0, orphan: true },
    ]);
  });

  /** A payload whose vectors disagree in length is a server contract
   *  violation. Rendering the shorter prefix silently would show a table that
   *  looks complete; refusing the whole thing is the honest response. */
  it("refuses a payload whose vectors do not line up", () => {
    // Both vectors, not just the first: a check that only compares one leaves
    // the other free to disagree silently.
    expect(() => connectivityRows(analytics({ inDegree: [0, 1] }))).toThrow(/in-degrees/);
    expect(() => connectivityRows(analytics({ outDegree: [0, 1] }))).toThrow(/out-degrees/);
    // The message names the counts, because an operator reading it needs to
    // know which side is short.
    expect(() => connectivityRows(analytics({ inDegree: [0, 1] }))).toThrow(/3 nodes/);
  });

  /** Ranked most-connected first: the reason to open this panel is "what is
   *  the hub here", and a list in walk order buries the answer. */
  it("puts the most connected node first", () => {
    const rows = connectivityRows(
      analytics({ inDegree: [0, 9, 0], outDegree: [1, 0, 0] }),
    );
    expect(rows[0]!.id).toBe("1:b");
  });

  /** **Ranked on in *plus* out, not on their difference.** A node with traffic
   *  both ways is the most connected thing in a neighbourhood; subtracting
   *  would rank it below a node with the same in-degree and no out-degree.
   *  Written to kill a mutant that swapped the `+` for a `-`. */
  it("ranks a node with traffic both ways above one with traffic one way", () => {
    const rows = connectivityRows(
      analytics({
        nodes: ["1:oneWay", "1:bothWays"],
        inDegree: [4, 4],
        outDegree: [0, 3],
        orphans: [],
      }),
    );
    expect(rows.map((r) => r.id)).toEqual(["1:bothWays", "1:oneWay"]);
  });

  it("has nothing to show for an empty neighbourhood", () => {
    expect(
      connectivityRows(
        analytics({ nodes: [], inDegree: [], outDegree: [], orphans: [] }),
      ),
    ).toEqual([]);
  });
});

describe("stating what the numbers cover", () => {
  /** **A truncated walk presented as complete is the failure this project
   *  refuses everywhere.** "3 nodes" from a walk that stopped early is a
   *  claim the server never made. */
  it("says so when the walk stopped early", () => {
    const text = describeAnalytics(analytics({ truncated: true }));
    expect(text.toLowerCase()).toContain("stopped");
  });

  it("does not hedge when the walk was complete", () => {
    const text = describeAnalytics(analytics());
    expect(text.toLowerCase()).not.toContain("stopped");
    expect(text).toContain("3");
  });

  /** Edge types come from the payload — the server derives them from the data
   *  it walked. A list compiled into the console would be a pack's vocabulary
   *  hardcoded, which the neutrality check fails the build over. */
  it("reports the edge types the payload named, whatever they are", () => {
    const text = describeAnalytics(analytics({ edgeTypes: ["prescribedIn", "coveredBy"] }));
    // Separated, not run together: `prescribedIncoveredBy` names no edge type
    // that exists and reads as one. Alphabetical rather than payload order, so
    // the sentence does not reorder between two requests that found the same
    // thing.
    expect(text).toContain("coveredBy, prescribedIn");
  });

  /** **Found in a browser: the summary read "connected by 1:parentSchema".**
   *  The payload renders edge types as `Sid`s and the namespace code is
   *  identity, not information — every one of them shares it, so it is pure
   *  noise in a sentence a human reads. The same stripping the filter's option
   *  list already does. */
  it("names an edge type without its namespace code", () => {
    const text = describeAnalytics(analytics({ edgeTypes: ["1:parentSchema", "1024:issuedBy"] }));
    expect(text).toContain("issuedBy, parentSchema");
    expect(text).not.toContain("1:");
    expect(text).not.toContain("1024:");
  });

  it("says so when the neighbourhood has no edges at all", () => {
    const text = describeAnalytics(analytics({ edgeTypes: [] }));
    expect(text.toLowerCase()).toContain("no relationship");
  });
});
