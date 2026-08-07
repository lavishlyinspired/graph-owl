import { describe, expect, it } from "vitest";
import {
  applyParseOutcome,
  initialEditorState,
  isSubsumptionPredicate,
  localName,
  namespaceOf,
  namespacesIn,
  predicatesIn,
  toOntologyElements,
  type RdfEditPreview,
} from "./ontologyDocument";

const NS = "https://graph-owl.dev/ns/catalog#";

function preview(overrides: Partial<RdfEditPreview> = {}): RdfEditPreview {
  return {
    triples: [
      { s: `${NS}Widget`, p: `${NS}name`, o: "A widget", oIsRef: false },
      {
        s: `${NS}Widget`,
        p: "http://www.w3.org/2000/01/rdf-schema#subClassOf",
        o: `${NS}Product`,
        oIsRef: true,
      },
    ],
    declared: [`${NS}Widget`],
    ...overrides,
  };
}

describe("applyParseOutcome — the RED test: the last good graph never disappears", () => {
  it("a syntax error updates the error and the document, but keeps lastGood untouched", () => {
    const good = preview();
    const withGraph = applyParseOutcome(initialEditorState(), "good text", {
      kind: "preview",
      preview: good,
    });
    expect(withGraph.lastGood).toEqual(good);

    const afterTypo = applyParseOutcome(withGraph, "good text bu", {
      kind: "syntaxError",
      message: "unexpected token",
      line: 3,
      column: 5,
    });

    // The whole point of this test: a half-typed edit must not blank the
    // picture the author was just looking at.
    expect(afterTypo.lastGood).toEqual(good);
    expect(afterTypo.error).toEqual({ message: "unexpected token", line: 3, column: 5 });
    expect(afterTypo.document).toBe("good text bu");
  });

  it("a successful parse clears any previous error", () => {
    const withError = applyParseOutcome(initialEditorState(), "bad", {
      kind: "syntaxError",
      message: "nope",
      line: 1,
      column: 1,
    });
    expect(withError.error).not.toBeNull();

    const fixed = applyParseOutcome(withError, "fixed", {
      kind: "preview",
      preview: preview(),
    });
    expect(fixed.error).toBeNull();
  });

  it("initial state has no graph and no error", () => {
    const state = initialEditorState();
    expect(state.lastGood).toBeNull();
    expect(state.error).toBeNull();
    expect(state.document).toBe("");
  });

  it("defaults to turtle when no format is given", () => {
    expect(initialEditorState().format).toBe("turtle");
  });

  it("an explicit format is kept, not overridden by the default", () => {
    expect(initialEditorState("jsonld").format).toBe("jsonld");
  });
});

describe("namespaceOf", () => {
  it("splits at the last # for a hash IRI", () => {
    expect(namespaceOf(`${NS}Widget`)).toBe(NS);
  });

  it("splits at the last / when there is no #", () => {
    expect(namespaceOf("https://example.org/ns/Widget")).toBe("https://example.org/ns/");
  });

  it("an IRI with neither returns itself, not an empty string", () => {
    expect(namespaceOf("urn:isbn:0451450523")).toBe("urn:isbn:0451450523");
  });
});

describe("localName", () => {
  it("strips the namespace, leaving only the local part", () => {
    expect(localName(`${NS}Widget`)).toBe("Widget");
  });

  it("an IRI with no namespace to strip returns itself whole", () => {
    expect(localName("urn:isbn:0451450523")).toBe("urn:isbn:0451450523");
  });

  it("an IRI that is exactly its own namespace (nothing after the separator) returns itself, not an empty string", () => {
    expect(localName(NS)).toBe(NS);
  });
});

describe("namespacesIn / predicatesIn — the filter option lists", () => {
  it("collects every distinct namespace across subjects, predicates, and ref objects, sorted", () => {
    const withADistinctRefNamespace = preview({
      triples: [
        ...preview().triples,
        {
          s: `${NS}Widget`,
          p: `${NS}madeBy`,
          o: "https://example.org/vendors#Acme",
          oIsRef: true,
        },
      ],
    });
    expect(namespacesIn(withADistinctRefNamespace)).toEqual([
      "http://www.w3.org/2000/01/rdf-schema#",
      "https://example.org/vendors#",
      NS,
    ]);
  });

  it("a literal object's own text is never treated as a namespace", () => {
    const namespaces = namespacesIn(preview());
    expect(namespaces).not.toContain("A widget");
  });

  it("a ref object's namespace is included only because it is a ref — disabling that check would silently drop it", () => {
    // `preview()`'s only ref object shares the base's own namespace, so a
    // mutant that skips ref objects entirely would not change this set —
    // this fixture's ref object is the one namespace nothing else supplies.
    const onlyReachableThroughTheRefObject = preview({
      triples: [
        {
          s: `${NS}Widget`,
          p: `${NS}madeBy`,
          o: "https://example.org/vendors#Acme",
          oIsRef: true,
        },
      ],
    });
    expect(namespacesIn(onlyReachableThroughTheRefObject)).toContain(
      "https://example.org/vendors#",
    );
  });

  it("collects every distinct predicate, deduplicated", () => {
    const withDuplicatePredicate = preview({
      triples: [
        ...preview().triples,
        { s: `${NS}Gadget`, p: `${NS}name`, o: "A gadget", oIsRef: false },
      ],
    });
    expect(predicatesIn(withDuplicatePredicate)).toEqual([
      "http://www.w3.org/2000/01/rdf-schema#subClassOf",
      `${NS}name`,
    ]);
  });
});

describe("isSubsumptionPredicate", () => {
  it("rdfs:subClassOf and rdfs:subPropertyOf are subsumption", () => {
    expect(isSubsumptionPredicate("http://www.w3.org/2000/01/rdf-schema#subClassOf")).toBe(true);
    expect(isSubsumptionPredicate("http://www.w3.org/2000/01/rdf-schema#subPropertyOf")).toBe(
      true,
    );
  });

  it("an ordinary property is not", () => {
    expect(isSubsumptionPredicate(`${NS}name`)).toBe(false);
  });
});

describe("toOntologyElements — the RED test: declared vs referenced must be visually distinguishable", () => {
  it("a declared subject and a referenced-only object get different classes", () => {
    const elements = toOntologyElements(preview(), { namespace: null, predicate: null });
    const widget = elements.find((e) => e.data.id === `${NS}Widget`);
    const product = elements.find((e) => e.data.id === `${NS}Product`);

    expect(widget?.classes).toBe("declared");
    expect(product?.classes).toBe("referenced");
  });

  it("a subsumption edge and a plain property edge get different classes", () => {
    const withBothEdgeKinds = preview({
      triples: [
        ...preview().triples,
        { s: `${NS}Widget`, p: `${NS}madeBy`, o: `${NS}Acme`, oIsRef: true },
      ],
    });
    const elements = toOntologyElements(withBothEdgeKinds, { namespace: null, predicate: null });
    const edges = elements.filter((e) => e.group === "edges");
    const subsumptionEdge = edges.find((e) => e.data.target === `${NS}Product`);
    const propertyEdge = edges.find((e) => e.data.target === `${NS}Acme`);

    expect(subsumptionEdge?.classes).toBe("subsumption");
    expect(propertyEdge?.classes).toBe("property");
  });

  it("a literal-valued triple never becomes an edge — only a ref object does", () => {
    const elements = toOntologyElements(preview(), { namespace: null, predicate: null });
    const edges = elements.filter((e) => e.group === "edges");
    // Only the subClassOf triple has oIsRef: true; the name triple must not
    // produce a second edge or a phantom "A widget" node.
    expect(edges).toHaveLength(1);
    expect(elements.some((e) => e.data.id === "A widget")).toBe(false);
  });

  it("every edge carries a real, non-empty id naming what it connects", () => {
    const elements = toOntologyElements(preview(), { namespace: null, predicate: null });
    const edge = elements.find((e) => e.group === "edges");
    expect(edge?.data.id).toBeTruthy();
    expect(edge?.data.id).toContain(`${NS}Widget`);
    expect(edge?.data.id).toContain(`${NS}Product`);
  });

  it("filtering by predicate keeps the matching edge and its endpoints, and drops the rest", () => {
    const withTwoEdges = preview({
      triples: [
        ...preview().triples,
        { s: `${NS}Widget`, p: `${NS}madeBy`, o: `${NS}Acme`, oIsRef: true },
      ],
    });
    const filtered = toOntologyElements(withTwoEdges, {
      namespace: null,
      predicate: "http://www.w3.org/2000/01/rdf-schema#subClassOf",
    });
    expect(filtered.some((e) => e.data.id === `${NS}Acme`)).toBe(false);
    expect(filtered.some((e) => e.data.id === `${NS}Product`)).toBe(true);
    expect(filtered.filter((e) => e.group === "edges")).toHaveLength(1);
  });

  it("filtering by namespace keeps only triples whose subject or predicate is in it", () => {
    const withAnotherNamespace = preview({
      triples: [
        ...preview().triples,
        {
          s: "https://example.org/vendors#Acme",
          p: "https://example.org/vendors#kind",
          o: "Vendor",
          oIsRef: false,
        },
      ],
    });
    const filtered = toOntologyElements(withAnotherNamespace, {
      namespace: NS,
      predicate: null,
    });
    expect(filtered.some((e) => e.data.id === "https://example.org/vendors#Acme")).toBe(false);
    expect(filtered.some((e) => e.data.id === `${NS}Widget`)).toBe(true);
  });

  it("a subject in the namespace keeps the triple even when the predicate is not", () => {
    const filtered = toOntologyElements(
      preview({
        triples: [
          {
            s: `${NS}Widget`,
            p: "https://example.org/vendors#kind",
            o: "Thing",
            oIsRef: false,
          },
        ],
      }),
      { namespace: NS, predicate: null },
    );
    expect(filtered.some((e) => e.data.id === `${NS}Widget`)).toBe(true);
  });

  it("a predicate in the namespace keeps the triple even when the subject is not", () => {
    const filtered = toOntologyElements(
      preview({
        triples: [
          {
            s: "https://example.org/vendors#Acme",
            p: `${NS}kind`,
            o: "Vendor",
            oIsRef: false,
          },
        ],
      }),
      { namespace: NS, predicate: null },
    );
    expect(filtered.some((e) => e.data.id === "https://example.org/vendors#Acme")).toBe(true);
  });
});
