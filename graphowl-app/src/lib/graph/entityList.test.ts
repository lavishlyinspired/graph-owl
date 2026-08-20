import { describe, expect, it } from "vitest";
import { allEntitiesQuery, entitiesFromSparqlRows } from "./entityList";

function row(s: string, type: string): Record<string, string> {
  return { s: `<${s}>`, type: `<${type}>` };
}

describe("the all-entities query", () => {
  it("scopes to one pack's own namespace", () => {
    expect(allEntitiesQuery("gst")).toBe(
      'SELECT ?s ?type WHERE { GRAPH ?g { ?s a ?type } FILTER(CONTAINS(STR(?type), "/packs/gst#")) }',
    );
  });
});

describe("turning raw rows into a picker's entity list", () => {
  const ns = "https://graph-owl.dev/packs/gst#";

  it("keeps a real instance, with its local id and type", () => {
    const entities = entitiesFromSparqlRows([row(`${ns}supplier-19AABCP8087C1ZV`, `${ns}Supplier`)]);
    expect(entities).toEqual([
      { id: "supplier-19AABCP8087C1ZV", iri: `${ns}supplier-19AABCP8087C1ZV`, type: "Supplier" },
    ]);
  });

  /** `gst:PurchaseInvoice a gst:Class` is the ontology's own declaration,
   *  not an invoice — a picker that included these would offer 18 classes
   *  and 33 properties as if they were things to explore, alongside the
   *  real invoices they describe. */
  it("excludes ontology-schema subjects (Class, Property)", () => {
    const entities = entitiesFromSparqlRows([
      row(`${ns}PurchaseInvoice`, `${ns}Class`),
      row(`${ns}taxAmount`, `${ns}Property`),
    ]);
    expect(entities).toEqual([]);
  });

  it("keeps one entry per subject even if it appears in more than one row", () => {
    const entities = entitiesFromSparqlRows([
      row(`${ns}supplier-1`, `${ns}Supplier`),
      row(`${ns}supplier-1`, `${ns}Supplier`),
    ]);
    expect(entities).toHaveLength(1);
  });

  it("sorts by type, then by id within a type", () => {
    const entities = entitiesFromSparqlRows([
      row(`${ns}supplier-2`, `${ns}Supplier`),
      row(`${ns}books-1`, `${ns}PurchaseInvoice`),
      row(`${ns}supplier-1`, `${ns}Supplier`),
    ]);
    expect(entities.map((e) => e.id)).toEqual(["books-1", "supplier-1", "supplier-2"]);
  });
});
