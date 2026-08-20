import { describe, expect, it } from "vitest";
import {
  looksLikeAssetId,
  nodeTypeQuery,
  openTargetFor,
  toGraphView,
  typesFromTypeRows,
  withNodeTypes,
  type RawGraphContext,
} from "./graphContext";

/** Factories, not fixtures: every test gets its own response, so one test
 *  cannot leave state behind that another depends on. */
function rawContext(overrides?: Partial<RawGraphContext>): RawGraphContext {
  return {
    nodes: [
      { id: "books-INV-006", iri: "https://graph-owl.dev/packs/gst#books-INV-006", label: null },
      { id: "supplier-19AABC", iri: "https://graph-owl.dev/packs/gst#supplier-19AABC", label: "Patel Chemicals & Co" },
    ],
    edges: [{ from: "books-INV-006", to: "supplier-19AABC", relationship: "issuedBy" }],
    truncated: false,
    ...overrides,
  };
}

describe("mapping a /graph/context response onto the console's graph model", () => {
  it("uses each node's IRI as its picture id, not the short local name the response keys edges by", () => {
    const view = toGraphView(rawContext());
    expect(view.nodes.map((n) => n.id)).toEqual([
      "https://graph-owl.dev/packs/gst#books-INV-006",
      "https://graph-owl.dev/packs/gst#supplier-19AABC",
    ]);
  });

  /** A follow-up expansion sends this id straight back to `/graph/context` as
   *  the new seed — `parse_node_id` on the server only accepts a UUID, a
   *  `namespace:local` pair, or a full IRI. The short local name the response
   *  keys nodes and edges by internally is none of those, so leaving it in
   *  place would make every second click 400. */
  it("rewrites edge endpoints to the same IRIs, so the picture stays internally consistent", () => {
    const view = toGraphView(rawContext());
    expect(view.edges).toEqual([
      {
        from: "https://graph-owl.dev/packs/gst#books-INV-006",
        to: "https://graph-owl.dev/packs/gst#supplier-19AABC",
        relationship: "issuedBy",
        derived: undefined,
      },
    ]);
  });

  it("falls back to the short id when a node has no resolvable IRI", () => {
    const view = toGraphView(
      rawContext({
        nodes: [{ id: "unresolved-1", iri: null, label: null }],
        edges: [],
      }),
    );
    expect(view.nodes[0]?.id).toBe("unresolved-1");
  });

  it("uses the resolved label as the node's display name", () => {
    const view = toGraphView(rawContext());
    expect(view.nodes[1]?.name).toBe("Patel Chemicals & Co");
  });

  it("falls back to the short id as the display name when there is no label", () => {
    const view = toGraphView(rawContext());
    expect(view.nodes[0]?.name).toBe("books-INV-006");
  });

  /** These nodes have no catalog asset behind them at all — a GST invoice is
   *  never one — so there is no `kind` to report. `null` here is the same
   *  "the reader may not see it, or nothing this catalog knows" reading
   *  `/assets/{id}/graph` already gives an unresolved node id. */
  it("carries no asset kind", () => {
    const view = toGraphView(rawContext());
    expect(view.nodes.every((n) => n.kind === null)).toBe(true);
  });

  it("passes the truncated flag through unchanged", () => {
    expect(toGraphView(rawContext({ truncated: true })).truncated).toBe(true);
  });
});

describe("choosing which neighbourhood endpoint a seed needs", () => {
  it("treats a bare UUID as a catalog asset id", () => {
    expect(looksLikeAssetId("3fa85f64-5717-4562-b3fc-2c963f66afa6")).toBe(true);
  });

  it("treats a full IRI as a graph-only subject, not a catalog asset", () => {
    expect(looksLikeAssetId("https://graph-owl.dev/packs/gst#books-INV-006")).toBe(false);
  });

  it("treats a namespace:local identifier as a graph-only subject, not a catalog asset", () => {
    expect(looksLikeAssetId("rdf:type")).toBe(false);
  });
});

describe("asking the graph what each node actually is", () => {
  it("binds every node IRI into one query, so N nodes cost one round trip", () => {
    const query = nodeTypeQuery([
      "https://graph-owl.dev/packs/gst#books-INV-006",
      "https://graph-owl.dev/packs/gst#supplier-19AABC",
    ]);
    expect(query).toContain("<https://graph-owl.dev/packs/gst#books-INV-006>");
    expect(query).toContain("<https://graph-owl.dev/packs/gst#supplier-19AABC>");
    expect(query).toContain("VALUES");
  });

  /** **`GRAPH ?g` is required, and its absence fails silently.** Imports land
   *  in named graphs, not the default one, so a bare `?s a ?t` pattern matches
   *  nothing and returns zero rows — which reads as "these nodes have no
   *  type" rather than as a broken query. */
  it("matches inside named graphs, not only the default graph", () => {
    expect(nodeTypeQuery(["https://example.org/a"])).toContain("GRAPH");
  });

  it("asks nothing when there are no nodes to ask about", () => {
    expect(nodeTypeQuery([])).toBeNull();
  });

  it("reads a subject's type out of the result rows", () => {
    const types = typesFromTypeRows([
      { s: "<https://graph-owl.dev/packs/gst#supplier-19AABC>", t: "<https://graph-owl.dev/packs/gst#Supplier>" },
    ]);
    expect(types.get("https://graph-owl.dev/packs/gst#supplier-19AABC")).toBe("gst:Supplier");
  });

  /** The same subject is typed once per named graph it appears in, so a
   *  supplier in four import graphs comes back four identical rows. Counting
   *  them as four types would put four identical entries in the legend. */
  it("collapses the repeat rows one subject gets from several named graphs", () => {
    const types = typesFromTypeRows([
      { s: "<https://ex.org/s>", t: "<https://graph-owl.dev/packs/gst#Supplier>" },
      { s: "<https://ex.org/s>", t: "<https://graph-owl.dev/packs/gst#Supplier>" },
      { s: "<https://ex.org/s>", t: "<https://graph-owl.dev/packs/gst#Supplier>" },
    ]);
    expect(types.size).toBe(1);
    expect(types.get("https://ex.org/s")).toBe("gst:Supplier");
  });

  /** A subject genuinely carrying two classes must resolve the same way on
   *  every load — an unstable pick would recolour the node between refreshes
   *  and move its legend entry. */
  it("picks one type deterministically when a subject carries several", () => {
    const rows = [
      { s: "<https://ex.org/s>", t: "<https://graph-owl.dev/packs/gst#Zebra>" },
      { s: "<https://ex.org/s>", t: "<https://graph-owl.dev/packs/gst#Alpha>" },
    ];
    const forward = typesFromTypeRows(rows);
    const reversed = typesFromTypeRows([...rows].reverse());
    expect(forward.get("https://ex.org/s")).toBe(reversed.get("https://ex.org/s"));
  });

  it("keeps the bare local name when the IRI carries no pack prefix", () => {
    const types = typesFromTypeRows([{ s: "<https://ex.org/s>", t: "<https://ex.org/Thing>" }]);
    expect(types.get("https://ex.org/s")).toBe("Thing");
  });

  it("ignores a row missing either half of the fact", () => {
    const types = typesFromTypeRows([{ s: "<https://ex.org/s>" }, { t: "<https://ex.org/T>" }]);
    expect(types.size).toBe(0);
  });
});

describe("attaching resolved types to a picture", () => {
  it("carries the type onto the matching node so the canvas can colour it", () => {
    const view = toGraphView(rawContext());
    const typed = withNodeTypes(
      view,
      new Map([["https://graph-owl.dev/packs/gst#supplier-19AABC", "gst:Supplier"]]),
    );
    expect(typed.nodes[1]?.semanticType).toBe("gst:Supplier");
  });

  it("leaves a node the graph could not type alone rather than guessing one", () => {
    const view = toGraphView(rawContext());
    const typed = withNodeTypes(view, new Map());
    expect(typed.nodes[0]?.semanticType).toBeUndefined();
  });

  it("does not disturb the edges", () => {
    const view = toGraphView(rawContext());
    expect(withNodeTypes(view, new Map()).edges).toEqual(view.edges);
  });
});

/** **Provenance the response already carried and the mapping threw away.**
 *  `/graph/context` returns, per node, the import graphs the subject appears
 *  in. That is real, checkable provenance — the one thing the detail panel
 *  can say about where a fact came from without inventing it. */
describe("carrying provenance through", () => {
  it("keeps the import graphs a subject was seen in", () => {
    const view = toGraphView(
      rawContext({
        nodes: [
          {
            id: "supplier-1",
            iri: "https://ex.org/supplier-1",
            label: null,
            sources: ["reco-aaa-books", "reco-bbb-gstr2b"],
          },
        ],
        edges: [],
      }),
    );
    expect(view.nodes[0]?.sources).toEqual(["reco-aaa-books", "reco-bbb-gstr2b"]);
  });

  it("reports no sources rather than an empty list when the response carried none", () => {
    expect(toGraphView(rawContext()).nodes[0]?.sources).toBeUndefined();
  });
});

/** **Where "open this node" should actually go.**
 *
 *  `/entity/:id` was originally its own page and loaded only a *catalog
 *  asset* — it called `fetchAsset`, `fetchAssetVersions` and
 *  `fetchContradictions`, all of which took a UUID, so sending a
 *  graph-only subject (a GST invoice IRI) there made every one of those
 *  400 and the button read as doing nothing. `EntityPanel` (the same
 *  content, now embedded as Explore's own Entity tab) branches the same
 *  way Explore's own graph fetch does — a catalog asset through the asset
 *  endpoints, anything else through `/graph/context` and `/findings` — so
 *  both id shapes land somewhere real. */
describe("choosing where a node opens", () => {
  it("opens a catalog asset on Explore's own Entity tab", () => {
    expect(openTargetFor("3fa85f64-5717-4562-b3fc-2c963f66afa6")).toBe(
      "/explore/3fa85f64-5717-4562-b3fc-2c963f66afa6?view=entity",
    );
  });

  it("opens a graph-only subject on the same Entity tab", () => {
    expect(openTargetFor("https://graph-owl.dev/packs/gst#books-INV-006")).toBe(
      "/explore/https%3A%2F%2Fgraph-owl.dev%2Fpacks%2Fgst%23books-INV-006?view=entity",
    );
  });

  it("escapes an identifier so its slashes cannot be read as path segments", () => {
    expect(openTargetFor("https://ex.org/a/b")).not.toContain("ex.org/a/b");
  });
});
