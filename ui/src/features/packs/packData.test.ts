/** The loaded import sources behind the Explore "Pack data" block — Plan 115
 *  Slice B1.
 *
 *  **The point of the block is discoverability**: a CA who uploads a file gets
 *  a toast naming the source (C1), and Explore must show that same name under
 *  the pack that produced it. The parsing is the risk here — a graph IRI and
 *  an integer literal both arrive N-Triples-shaped — so it lives in pure
 *  functions and is tested with hand-written rows, not a live graph. */

import { describe, expect, it } from "vitest";
import {
  importSourceOf,
  loadedSourcesFromSparql,
  sourcesForPack,
  type LoadedSource,
} from "./packData";

describe("an import graph's source name", () => {
  it("reads the source out of the graph IRI", () => {
    expect(importSourceOf("graph:import:gst-gstr2b-2025-07")).toBe("gst-gstr2b-2025-07");
  });

  it("is not every named graph, and not an empty import graph", () => {
    expect(importSourceOf("graph:vocab")).toBeNull();
    expect(importSourceOf("graph:import:")).toBeNull();
  });
});

describe("the named-graph listing", () => {
  it("parses import graphs into sources with their triple counts", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<graph:import:gst-books-2025-07>", n: '"42"^^<http://www.w3.org/2001/XMLSchema#integer>' },
      { g: "<graph:import:gst-gstr2b-2025-07>", n: '"168"^^<http://www.w3.org/2001/XMLSchema#integer>' },
    ]);

    expect(sources).toEqual([
      { name: "gst-books-2025-07", packId: "gst", triples: 42 },
      { name: "gst-gstr2b-2025-07", packId: "gst", triples: 168 },
    ]);
  });

  /** A vocabulary or derived graph is a real named graph and is deliberately
   *  not offered as "data you imported" — it was not uploaded by anybody. */
  it("drops graphs that are not import graphs", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<graph:import:gst-books-2025-07>", n: '"42"^^<…integer>' },
      { g: "<graph:vocab>", n: '"9"^^<…integer>' },
    ]);

    expect(sources).toEqual([{ name: "gst-books-2025-07", packId: "gst", triples: 42 }]);
  });

  it("orders the listing by source name, so a CA can scan it", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<graph:import:gst-gstr2b-2025-07>", n: '"168"^^<…integer>' },
      { g: "<graph:import:gst-books-2025-07>", n: '"42"^^<…integer>' },
    ]);

    expect(sources.map((s) => s.name)).toEqual(["gst-books-2025-07", "gst-gstr2b-2025-07"]);
  });
});

describe("filing sources under their pack", () => {
  const sources: readonly LoadedSource[] = [
    { name: "gst-books-2025-07", packId: "gst", triples: 42 },
    { name: "gst-gstr2b-2025-07", packId: "gst", triples: 168 },
    { name: "erpnext-orders", packId: "erpnext", triples: 9 },
  ];

  it("keeps only the pack's own sources", () => {
    expect(sourcesForPack(sources, "gst").map((s) => s.name)).toEqual([
      "gst-books-2025-07",
      "gst-gstr2b-2025-07",
    ]);
  });

  it("reports nothing for a pack with no data loaded", () => {
    expect(sourcesForPack(sources, "hospitality")).toEqual([]);
  });
});
