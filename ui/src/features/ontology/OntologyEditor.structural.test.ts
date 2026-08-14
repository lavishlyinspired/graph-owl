/** The ontology editor's pack loader — Plan 116 Slice A.
 *
 *  Structural, like `PackDataExplorer.structural.test.ts`: the picker renders
 *  only against a real graph (`/namespaces` + a named-graph query, then a
 *  triples read of the chosen source), and what it *computes* — which source
 *  is a pack's ontology, and how its rows become a document — is pure and
 *  already unit-tested in `packData.test.ts`. This pins the wiring: that the
 *  picker reads the same two sources of truth `PackDataExplorer` reads, that
 *  it resolves a pack's ontology source rather than any source, and that a
 *  load lands as N-Triples in the same state the manual paste path writes to. */

import { describe, expect, it } from "vitest";
import source from "./OntologyEditor.tsx?raw";

describe("the ontology editor's installed-pack loader", () => {
  it("reads the same two sources of truth the Explore Pack data block reads", () => {
    expect(source).toMatch(/api\.namespaces\(\)/);
    expect(source).toMatch(/installedPacks/);
    expect(source).toMatch(/loadedSourcesFromSparql/);
    expect(source).toMatch(/NAMED_GRAPHS_QUERY/);
  });

  it("resolves a pack's own ontology source, not just any of its sources", () => {
    expect(source).toMatch(/ontologySourceFor/);
  });

  it("loads the source's whole graph and formats it back into N-Triples text", () => {
    expect(source).toMatch(/triplesQuery/);
    expect(source).toMatch(/ntriplesFromRows/);
  });

  it("writes a load into the same state the manual paste path already drives", () => {
    expect(source).toMatch(/format:\s*"ntriples"/);
    expect(source).toMatch(/setState/);
  });
});
