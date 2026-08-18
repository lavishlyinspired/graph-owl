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
  localNameOf,
  ntriplesFromRows,
  ontologySourceFor,
  sourcesForPack,
  subjectsFromSparql,
  subjectsQuery,
  triplesQuery,
  typesQuery,
  type LoadedSource,
} from "./packData";

describe("an import graph's source name", () => {
  it("reads the source out of the graph IRI", () => {
    expect(importSourceOf("graph:import:gst-gstr2b-2025-07")).toBe("gst-gstr2b-2025-07");
  });

  /** **The shape the endpoint actually returns.** A `Sid` is
   *  `dsc:{id}`, and `/sparql` renders one as its full IRI —
   *  `https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2026-08`
   *  (`graph-owl-core/src/flake.rs`). The bare `graph:import:` form above is
   *  the server's internal address; the block must name the source out of
   *  what the wire actually carries, not out of an internal address it never
   *  sees. This was a live bug: the listing matched only the bare form,
   *  every real graph was dropped, and the block read "Nothing imported
   *  yet" against a fully-loaded pack. */
  it("reads the source out of the namespaced IRI the wire actually returns", () => {
    expect(
      importSourceOf("https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2026-08"),
    ).toBe("gst-gstr2b-2026-08");
  });

  it("is not every named graph, and not an empty import graph", () => {
    expect(importSourceOf("graph:vocab")).toBeNull();
    expect(importSourceOf("https://graph-owl.dev/ns/catalog#graph:vocab")).toBeNull();
    expect(importSourceOf("graph:import:")).toBeNull();
  });
});

describe("the named-graph listing", () => {
  it("parses import graphs into sources with their triple counts", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-books-2025-07>", n: '"42"^^<http://www.w3.org/2001/XMLSchema#integer>' },
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07>", n: '"168"^^<http://www.w3.org/2001/XMLSchema#integer>' },
    ]);

    expect(sources).toEqual([
      {
        name: "gst-books-2025-07",
        packId: "gst",
        iri: "https://graph-owl.dev/ns/catalog#graph:import:gst-books-2025-07",
        triples: 42,
      },
      {
        name: "gst-gstr2b-2025-07",
        packId: "gst",
        iri: "https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07",
        triples: 168,
      },
    ]);
  });

  /** The source keeps the IRI the graph reported, so the source view can
   *  query that exact graph by what the wire said — not by re-assembling the
   *  IRI from the name and a hardcoded catalog prefix. */
  it("carries the graph IRI alongside the parsed source name", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07>", n: '"2"^^<…integer>' },
    ]);

    expect(sources[0]?.iri).toBe(
      "https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07",
    );
  });

  /** A vocabulary or derived graph is a real named graph and is deliberately
   *  not offered as "data you imported" — it was not uploaded by anybody. */
  it("drops graphs that are not import graphs", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-books-2025-07>", n: '"42"^^<…integer>' },
      { g: "<https://graph-owl.dev/ns/catalog#graph:vocab>", n: '"9"^^<…integer>' },
    ]);

    expect(sources).toEqual([
      {
        name: "gst-books-2025-07",
        packId: "gst",
        iri: "https://graph-owl.dev/ns/catalog#graph:import:gst-books-2025-07",
        triples: 42,
      },
    ]);
  });

  it("orders the listing by source name, so a CA can scan it", () => {
    const sources = loadedSourcesFromSparql([
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07>", n: '"168"^^<…integer>' },
      { g: "<https://graph-owl.dev/ns/catalog#graph:import:gst-books-2025-07>", n: '"42"^^<…integer>' },
    ]);

    expect(sources.map((s) => s.name)).toEqual(["gst-books-2025-07", "gst-gstr2b-2025-07"]);
  });
});

describe("a source's subjects", () => {
  it("reads the local name out of a subject IRI", () => {
    expect(localNameOf("https://graph-owl.dev/packs/gst#2b-INV-1010")).toBe("2b-INV-1010");
    expect(localNameOf("https://graph-owl.dev/packs/gst#Gstr2bInvoice")).toBe("Gstr2bInvoice");
  });

  it("lists every subject with its type and triple count, ordered for scanning", () => {
    const subjects = subjectsFromSparql(
      [
        { s: "<https://graph-owl.dev/packs/gst#2b-INV-1011>", n: '"9"^^<…integer>' },
        { s: "<https://graph-owl.dev/packs/gst#2b-INV-1010>", n: '"8"^^<…integer>' },
        { s: "<https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM>", n: '"5"^^<…integer>' },
      ],
      [
        { s: "<https://graph-owl.dev/packs/gst#2b-INV-1010>", t: "<https://graph-owl.dev/packs/gst#Gstr2bInvoice>" },
        { s: "<https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM>", t: "<https://graph-owl.dev/packs/gst#Supplier>" },
      ],
    );

    expect(subjects).toEqual([
      { iri: "https://graph-owl.dev/packs/gst#2b-INV-1010", localName: "2b-INV-1010", kind: "Gstr2bInvoice", triples: 8 },
      { iri: "https://graph-owl.dev/packs/gst#2b-INV-1011", localName: "2b-INV-1011", kind: null, triples: 9 },
      { iri: "https://graph-owl.dev/packs/gst#supplier-27AABCU9603R1ZM", localName: "supplier-27AABCU9603R1ZM", kind: "Supplier", triples: 5 },
    ]);
  });

  it("keeps its own subject even when the type query answers nothing", () => {
    const subjects = subjectsFromSparql(
      [{ s: "<https://graph-owl.dev/packs/gst#2b-INV-1010>", n: '"8"^^<…integer>' }],
      [],
    );

    expect(subjects).toEqual([
      { iri: "https://graph-owl.dev/packs/gst#2b-INV-1010", localName: "2b-INV-1010", kind: null, triples: 8 },
    ]);
  });

  it("attaches each subject only its own type, never a sibling's", () => {
    const subjects = subjectsFromSparql(
      [
        { s: "<https://graph-owl.dev/packs/gst#2b-INV-1010>", n: '"8"^^<…integer>' },
        { s: "<https://graph-owl.dev/packs/gst#2b-INV-9999>", n: '"3"^^<…integer>' },
      ],
      [{ s: "<https://graph-owl.dev/packs/gst#2b-INV-9999>", t: "<https://graph-owl.dev/packs/gst#ForeignType>" }],
    );

    expect(subjects).toEqual([
      { iri: "https://graph-owl.dev/packs/gst#2b-INV-1010", localName: "2b-INV-1010", kind: null, triples: 8 },
      { iri: "https://graph-owl.dev/packs/gst#2b-INV-9999", localName: "2b-INV-9999", kind: "ForeignType", triples: 3 },
    ]);
  });
});

describe("the source view's queries", () => {
  const iri = "https://graph-owl.dev/ns/catalog#graph:import:gst-gstr2b-2025-07";

  it("scopes the subject listing to one source's own graph", () => {
    expect(subjectsQuery(iri)).toContain(`GRAPH <${iri}>`);
    expect(subjectsQuery(iri)).toContain("GROUP BY ?s");
  });

  it("scopes the type listing to the same graph", () => {
    expect(typesQuery(iri)).toContain(`GRAPH <${iri}>`);
  });

  it("scopes a plain triples listing to the same graph — Plan 116 Slice A's ontology load", () => {
    expect(triplesQuery(iri)).toContain(`GRAPH <${iri}>`);
    expect(triplesQuery(iri)).toContain("?s ?p ?o");
  });
});

describe("filing sources under their pack", () => {
  const sources: readonly LoadedSource[] = [
    { name: "gst-books-2025-07", packId: "gst", iri: "…#graph:import:gst-books-2025-07", triples: 42 },
    { name: "gst-gstr2b-2025-07", packId: "gst", iri: "…#graph:import:gst-gstr2b-2025-07", triples: 168 },
    { name: "erpnext-orders", packId: "erpnext", iri: "…#graph:import:erpnext-orders", triples: 9 },
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

describe("formatting a source's own triples as N-Triples — Plan 116 Slice A", () => {
  it("joins a plain SELECT ?s ?p ?o row into one well-formed line", () => {
    expect(
      ntriplesFromRows([
        {
          s: "<https://graph-owl.dev/packs/gst#GoodsReceipt>",
          p: "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>",
          o: "<https://graph-owl.dev/packs/gst#Class>",
        },
      ]),
    ).toBe(
      "<https://graph-owl.dev/packs/gst#GoodsReceipt> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://graph-owl.dev/packs/gst#Class> .",
    );
  });

  it("passes a language-tagged or typed literal object through unchanged — the wire already delivers N-Triples lexical form", () => {
    expect(
      ntriplesFromRows([
        {
          s: "<https://graph-owl.dev/packs/gst#GoodsReceipt>",
          p: "<https://graph-owl.dev/packs/gst#label>",
          o: '"Goods or services receipt event"',
        },
      ]),
    ).toBe(
      '<https://graph-owl.dev/packs/gst#GoodsReceipt> <https://graph-owl.dev/packs/gst#label> "Goods or services receipt event" .',
    );
  });

  it("joins multiple rows with one line each, in the rows' own order", () => {
    const text = ntriplesFromRows([
      { s: "<urn:a>", p: "<urn:p1>", o: "<urn:b>" },
      { s: "<urn:a>", p: "<urn:p2>", o: '"2"^^<http://www.w3.org/2001/XMLSchema#integer>' },
    ]);
    expect(text.split("\n")).toEqual([
      "<urn:a> <urn:p1> <urn:b> .",
      '<urn:a> <urn:p2> "2"^^<http://www.w3.org/2001/XMLSchema#integer> .',
    ]);
  });

  it("reports the empty string for no rows, not a stray newline or literal 'undefined'", () => {
    expect(ntriplesFromRows([])).toBe("");
  });

  it("skips a row missing any of s, p or o rather than emitting a malformed line", () => {
    expect(ntriplesFromRows([{ s: "<urn:a>", p: "<urn:p>" }])).toBe("");
  });
});

describe("finding a pack's own ontology source — Plan 116 Slice A", () => {
  const sources: readonly LoadedSource[] = [
    { name: "gst-ontology", packId: "gst", iri: "…#graph:import:gst-ontology", triples: 56 },
    { name: "gst-gstr2b-2025-07", packId: "gst", iri: "…#graph:import:gst-gstr2b-2025-07", triples: 168 },
  ];

  it("picks the source named by the pack-plus-ontology convention every shipped pack.toml follows", () => {
    expect(ontologySourceFor("gst", sources)?.name).toBe("gst-ontology");
  });

  it("reports null for a pack with no ontology source loaded, rather than falling back to some other source", () => {
    expect(ontologySourceFor("hospitality", sources)).toBeNull();
  });

  it("does not match another pack's ontology source by name alone", () => {
    const crossPack: readonly LoadedSource[] = [
      { name: "hospitality-ontology", packId: "hospitality", iri: "…#graph:import:hospitality-ontology", triples: 12 },
    ];
    expect(ontologySourceFor("gst", crossPack)).toBeNull();
  });
});
