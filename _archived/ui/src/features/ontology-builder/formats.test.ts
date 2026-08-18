import { describe, expect, it } from "vitest";
import {
  exportJsonLd,
  exportNTriples,
  exportOwl,
  exportTtl,
  importJsonLd,
  importNTriples,
  importTtl,
} from "./formats";
import { EMPTY_MODEL, addEntityType, addRelationship } from "./state";
import type { OntologyModel } from "./types";

function sampleModel(): OntologyModel {
  let model = addEntityType(EMPTY_MODEL, {
    name: "Customer",
    displayName: "Customer",
    description: "A customer",
    color: "#2A78D6",
    namespace: null,
  });
  const customerId = model.entityTypes[0]!.id;
  model = addEntityType(model, {
    name: "Order",
    displayName: "Order",
    description: "A purchase order",
    color: "#16A34A",
    namespace: null,
  });
  const orderId = model.entityTypes[1]!.id;
  model = addRelationship(model, {
    fromEntityTypeId: customerId,
    toEntityTypeId: orderId,
    name: "places",
    displayName: "Places",
    description: "Customer places order",
    cardinality: "oneToMany",
  });
  return model;
}

describe("export formats", () => {
  it("exports TTL", () => {
    const ttl = exportTtl(sampleModel(), "https://example.org/");
    expect(ttl).toContain("@prefix ex: <https://example.org/> .");
    expect(ttl).toContain("ex:Customer");
    expect(ttl).toContain("ex:Order");
    expect(ttl).toContain("ex:places");
    expect(ttl).toContain("a owl:Class");
    expect(ttl).toContain("a owl:ObjectProperty");
  });

  it("exports JSON-LD", () => {
    const jsonld = exportJsonLd(sampleModel(), "https://example.org/");
    const parsed = JSON.parse(jsonld);
    expect(parsed["@context"]).toBeDefined();
    expect(parsed["@graph"]).toHaveLength(3);
  });

  it("exports N-Triples", () => {
    const nt = exportNTriples(sampleModel(), "https://example.org/");
    expect(nt).toContain("<https://example.org/Customer>");
    expect(nt).toContain("<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>");
    expect(nt).toContain("<http://www.w3.org/2002/07/owl#Class>");
  });

  it("exports OWL/RDF XML", () => {
    const owl = exportOwl(sampleModel(), "https://example.org/");
    expect(owl).toContain('<?xml version="1.0"');
    expect(owl).toContain("<owl:Class");
    expect(owl).toContain("<owl:ObjectProperty");
    expect(owl).toContain("Customer");
  });
});

describe("import formats", () => {
  it("imports TTL classes and properties", () => {
    const ttl = `
      @prefix ex: <https://example.org/> .
      @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
      ex:Customer a <http://www.w3.org/2002/07/owl#Class> ;
        rdfs:label "Customer" ;
        rdfs:comment "A customer" .
      ex:Order a <http://www.w3.org/2002/07/owl#Class> ;
        rdfs:label "Order" .
      ex:places a <http://www.w3.org/2002/07/owl#ObjectProperty> ;
        rdfs:label "Places" ;
        rdfs:domain ex:Customer ;
        rdfs:range ex:Order .
    `;
    const model = importTtl(ttl);
    expect(model.entityTypes).toHaveLength(2);
    expect(model.relationships).toHaveLength(1);
    expect(model.entityTypes[0]!.name).toBe("Customer");
    expect(model.relationships[0]!.name).toBe("places");
  });

  it("imports JSON-LD", () => {
    const jsonld = JSON.stringify({
      "@context": { ex: "https://example.org/" },
      "@graph": [
        {
          "@id": "ex:Customer",
          "@type": "owl:Class",
          label: "Customer",
          comment: "A customer",
        },
        {
          "@id": "ex:Order",
          "@type": "owl:Class",
          label: "Order",
        },
      ],
    });
    const model = importJsonLd(jsonld);
    expect(model.entityTypes).toHaveLength(2);
    expect(model.entityTypes[0]!.name).toBe("Customer");
  });

  it("gives each imported JSON-LD class a different colour when the source carries none", () => {
    const jsonld = JSON.stringify({
      "@context": { ex: "https://example.org/" },
      "@graph": [
        { "@id": "ex:Customer", "@type": "owl:Class", label: "Customer" },
        { "@id": "ex:Order", "@type": "owl:Class", label: "Order" },
      ],
    });
    const model = importJsonLd(jsonld);
    expect(model.entityTypes[0]!.color).not.toBe(model.entityTypes[1]!.color);
  });

  it("imports N-Triples", () => {
    const nt = `
      <https://example.org/Customer> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#Class> .
      <https://example.org/Customer> <http://www.w3.org/2000/01/rdf-schema#label> "Customer" .
    `;
    const model = importNTriples(nt);
    expect(model.entityTypes).toHaveLength(1);
    expect(model.entityTypes[0]!.name).toBe("Customer");
  });

  it("gives each imported class a different colour from the standard palette when the source carries none", () => {
    const nt = `
      <https://example.org/Customer> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#Class> .
      <https://example.org/Order> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#Class> .
    `;
    const model = importNTriples(nt);
    expect(model.entityTypes).toHaveLength(2);
    expect(model.entityTypes[0]!.color).not.toBe(model.entityTypes[1]!.color);
  });

  it("still honours an explicit colour hint (rdfs:seeAlso) when the source provides one", () => {
    const nt = `
      <https://example.org/Customer> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#Class> .
      <https://example.org/Customer> <http://www.w3.org/2000/01/rdf-schema#seeAlso> "#123456" .
    `;
    const model = importNTriples(nt);
    expect(model.entityTypes[0]!.color).toBe("#123456");
  });
});
