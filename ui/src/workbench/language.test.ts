import { describe, expect, it } from "vitest";
import { defaultQuery, isLanguage, keepDrafts, pastTenseNote } from "./language";

describe("choosing a query language", () => {
  it("recognises only the two the server actually implements", () => {
    expect(isLanguage("sparql")).toBe(true);
    expect(isLanguage("cypher")).toBe(true);
    expect(isLanguage("gremlin")).toBe(false);
    expect(isLanguage(null)).toBe(false);
  });

  /** A blank editor is a worse starting point than a query that runs: the
   *  first thing anyone needs from a query surface is proof it answers. */
  it("starts each language with something that runs", () => {
    expect(defaultQuery("sparql")).toContain("SELECT");
    expect(defaultQuery("cypher")).toContain("MATCH");
  });
});

describe("switching between them", () => {
  /** **Switching language must not throw away what was typed.** A reviewer
   *  who toggles to look at the other language and back has lost real work
   *  otherwise — and the loss is silent, which is the part that makes it
   *  unforgivable rather than merely annoying. */
  it("remembers each language's draft across a switch", () => {
    const after = keepDrafts({}, "sparql", "SELECT ?s WHERE { ?s ?p ?o }");
    expect(keepDrafts(after, "cypher", "MATCH (n) RETURN n").sparql).toBe(
      "SELECT ?s WHERE { ?s ?p ?o }",
    );
  });

  it("offers the default when a language has no draft yet", () => {
    const drafts = keepDrafts({}, "sparql", "SELECT ?s WHERE { ?s ?p ?o }");
    expect(drafts.cypher ?? defaultQuery("cypher")).toBe(defaultQuery("cypher"));
  });
});

describe("saying which graph was queried", () => {
  /** **A result from the past that looks like a result from now is the one
   *  failure this surface cannot afford.** Historical data and stale data are
   *  indistinguishable on screen unless something says which this is. */
  it("names the instant when the clock is set, and says nothing when it is not", () => {
    expect(pastTenseNote(null)).toBeNull();
    const note = pastTenseNote("2026-07-31T00:00:00Z");
    expect(note).not.toBeNull();
    expect(note!.toLowerCase()).toContain("as it stood");
  });
});
