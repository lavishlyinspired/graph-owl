import { describe, expect, it } from "vitest";
import { ontologyGraphQuery, ontologyModelFromSparqlRows } from "./ontologyModel";

const RDF_TYPE = "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>";
const OWL_OBJECT_PROPERTY = "<http://www.w3.org/2002/07/owl#ObjectProperty>";
const RDFS_DOMAIN = "<http://www.w3.org/2000/01/rdf-schema#domain>";
const RDFS_RANGE = "<http://www.w3.org/2000/01/rdf-schema#range>";

function row(s: string, p: string, o: string): Record<string, string> {
  return { s, p, o };
}

function classRow(iri: string): Record<string, string> {
  return row(`<${iri}>`, RDF_TYPE, "<https://graph-owl.dev/packs/gst#Class>");
}

function labelRow(iri: string, text: string): Record<string, string> {
  return row(`<${iri}>`, "<https://graph-owl.dev/packs/gst#label>", `"${text}"`);
}

function relationshipRows(
  propIri: string,
  from: string,
  to: string,
  label: string,
): readonly Record<string, string>[] {
  return [
    row(`<${propIri}>`, RDF_TYPE, OWL_OBJECT_PROPERTY),
    row(`<${propIri}>`, RDFS_DOMAIN, `<${from}>`),
    row(`<${propIri}>`, RDFS_RANGE, `<${to}>`),
    row(`<${propIri}>`, "<https://graph-owl.dev/packs/gst#label>", `"${label}"`),
  ];
}

describe("the ontology graph's own SPARQL query", () => {
  /** Verified empirically against the running server before this was
   *  written: `SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s a gst:Class } }`
   *  returned exactly this IRI — not the `{packId}-ontology` "source name"
   *  alone, which is only the local part after `graph:import:`. */
  it("scopes to the pack's own ontology import graph", () => {
    expect(ontologyGraphQuery("gst")).toBe(
      "SELECT ?s ?p ?o WHERE { GRAPH <https://graph-owl.dev/ns/catalog#graph:import:gst-ontology> { ?s ?p ?o } }",
    );
  });

  it("scopes to a different pack's own graph for a different pack", () => {
    expect(ontologyGraphQuery("hospitality")).toContain(
      "https://graph-owl.dev/ns/catalog#graph:import:hospitality-ontology",
    );
  });
});

describe("building the ontology model from raw SPARQL rows", () => {
  it("reads every class declared under a pack's own namespace", () => {
    const model = ontologyModelFromSparqlRows([
      classRow("https://graph-owl.dev/packs/gst#Supplier"),
      classRow("https://graph-owl.dev/packs/gst#Invoice"),
    ]);
    expect(model.classes.map((c) => c.id)).toEqual([
      "https://graph-owl.dev/packs/gst#Supplier",
      "https://graph-owl.dev/packs/gst#Invoice",
    ]);
  });

  /** **Deliberately pinned, not "fixed."** A class is recognised by its
   *  `rdf:type` object's local name ending in `Class`, not by an exact
   *  match against `owl:Class` — every pack declares its *own*
   *  `{prefix}:Class` under its own namespace (`gst:Class`, `hosp:Class`),
   *  the same way each pack declares its own `{prefix}:label` rather than
   *  reusing `rdfs:label` (`packs/hospitality/ontology.ttl`'s own comment
   *  explains why: registering a term under RDFS's own namespace was
   *  refused). The suffix match is what makes this work for every pack's
   *  own vocabulary, not GST's alone — narrowing it to an exact
   *  `owl:Class` match would silently stop recognising every real pack's
   *  classes, since none of them use `owl:Class` at all. */
  it("recognises a pack-namespaced Class type, not only the standard owl:Class", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/hospitality#Guest>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/hospitality#Class>",
      ),
    ]);
    expect(model.classes.map((c) => c.id)).toEqual([
      "https://graph-owl.dev/packs/hospitality#Guest",
    ]);
  });

  /** The negative case a suffix match invites: a type IRI that merely ends
   *  in the same four letters without being a class declaration at all
   *  must not be swept in. */
  it("does not mistake an unrelated type ending in the same letters for a class", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/gst#Something>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/gst#NotAClass>",
      ),
    ]);
    expect(model.classes).toEqual([]);
  });

  it("names a class by its own label, not its bare IRI", () => {
    const model = ontologyModelFromSparqlRows([
      classRow("https://graph-owl.dev/packs/gst#FilingPeriod"),
      labelRow("https://graph-owl.dev/packs/gst#FilingPeriod", "Filing period"),
    ]);
    expect(model.classes[0]?.name).toBe("Filing period");
  });

  it("falls back to the IRI's own local name when a class has no label triple", () => {
    const model = ontologyModelFromSparqlRows([classRow("https://graph-owl.dev/packs/gst#Section")]);
    expect(model.classes[0]?.name).toBe("Section");
  });

  it("derives a class's namespace from the part of its IRI before the local name", () => {
    const model = ontologyModelFromSparqlRows([classRow("https://graph-owl.dev/packs/gst#Section")]);
    expect(model.classes[0]?.namespace).toBe("https://graph-owl.dev/packs/gst#");
  });

  it("reads a real relationship from an ObjectProperty's own domain, range and label", () => {
    const model = ontologyModelFromSparqlRows([
      classRow("https://graph-owl.dev/packs/gst#PurchaseInvoice"),
      classRow("https://graph-owl.dev/packs/gst#Supplier"),
      ...relationshipRows(
        "https://graph-owl.dev/packs/gst#issuedBy",
        "https://graph-owl.dev/packs/gst#PurchaseInvoice",
        "https://graph-owl.dev/packs/gst#Supplier",
        "issued by",
      ),
    ]);
    expect(model.relationships).toEqual([
      {
        id: "https://graph-owl.dev/packs/gst#issuedBy",
        label: "issued by",
        from: "https://graph-owl.dev/packs/gst#PurchaseInvoice",
        to: "https://graph-owl.dev/packs/gst#Supplier",
      },
    ]);
  });

  /** Matches `toG6Data`'s own rule in `graphModel.ts` for the instance
   *  graph: an edge to a node not present in the picture is dropped rather
   *  than drawn to nowhere. The ontology graph keeps the same rule for the
   *  same reason. */
  it("drops a relationship whose domain or range is not a known class", () => {
    const model = ontologyModelFromSparqlRows([
      classRow("https://graph-owl.dev/packs/gst#PurchaseInvoice"),
      ...relationshipRows(
        "https://graph-owl.dev/packs/gst#issuedBy",
        "https://graph-owl.dev/packs/gst#PurchaseInvoice",
        "https://graph-owl.dev/packs/gst#Supplier",
        "issued by",
      ),
    ]);
    expect(model.relationships).toEqual([]);
  });

  it("keeps a class that no relationship ever mentions, rather than dropping it", () => {
    const model = ontologyModelFromSparqlRows([classRow("https://graph-owl.dev/packs/gst#Policy")]);
    expect(model.classes).toHaveLength(1);
  });

  /** **The named, deferred gap, narrowed rather than closed.** A plain
   *  `gst:Property` triple has no `rdfs:domain` in this pack, so *which
   *  class it belongs to* is still not derivable — that half of the gap
   *  stands, and no relationship or class is fabricated from it. What this
   *  test pins is the other half: the property itself is real, declared
   *  data, and is no longer invisible just because its owner is unknown. */
  it("lists a plain gst:Property as a property, unattributed to any class", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/gst#taxAmount>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/gst#Property>",
      ),
      labelRow("https://graph-owl.dev/packs/gst#taxAmount", "tax amount"),
    ]);
    expect(model.properties).toEqual([
      {
        id: "https://graph-owl.dev/packs/gst#taxAmount",
        name: "tax amount",
        namespace: "https://graph-owl.dev/packs/gst#",
      },
    ]);
    expect(model.classes).toEqual([]);
    expect(model.relationships).toEqual([]);
  });

  /** Same reasoning as the Class suffix match above: every pack declares
   *  its own `{prefix}:Property` under its own namespace, not a shared
   *  term, so recognition has to go by local name rather than an exact
   *  IRI match against GST's own vocabulary. */
  it("recognises a pack-namespaced Property type, not only GST's own", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/hospitality#roomRate>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/hospitality#Property>",
      ),
    ]);
    expect(model.properties.map((p) => p.id)).toEqual([
      "https://graph-owl.dev/packs/hospitality#roomRate",
    ]);
  });

  it("does not mistake an unrelated type ending in the same letters for a property", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/gst#Something>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/gst#NotAProperty>",
      ),
    ]);
    expect(model.properties).toEqual([]);
  });

  it("falls back to the IRI's own local name when a property has no label triple", () => {
    const model = ontologyModelFromSparqlRows([
      row(
        "<https://graph-owl.dev/packs/gst#period>",
        RDF_TYPE,
        "<https://graph-owl.dev/packs/gst#Property>",
      ),
    ]);
    expect(model.properties[0]?.name).toBe("period");
  });
});
