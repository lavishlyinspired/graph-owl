import { describe, expect, it } from "vitest";
import { incomingReferencesQuery, outgoingFactsQuery } from "./entityQueries";

const IRI = "https://graph-owl.dev/packs/gst#books-11AABCZ9999A1Z1-INV-APR-013";

describe("real, copyable SPARQL for one entity", () => {
  it("asks for every fact this entity asserts, across all named graphs", () => {
    expect(outgoingFactsQuery(IRI)).toBe(`SELECT ?p ?o WHERE { GRAPH ?g { <${IRI}> ?p ?o } }`);
  });

  it("asks for everything that references this entity", () => {
    expect(incomingReferencesQuery(IRI)).toBe(`SELECT ?s ?p WHERE { GRAPH ?g { ?s ?p <${IRI}> } }`);
  });
});
