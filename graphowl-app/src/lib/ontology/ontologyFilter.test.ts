import { describe, expect, it } from "vitest";
import { matchesOntologyFilter } from "./ontologyFilter";

describe("filtering the classes/properties/relationships browser", () => {
  it("matches a name containing the query, case-insensitively", () => {
    expect(matchesOntologyFilter("Purchase invoice", "invoice")).toBe(true);
    expect(matchesOntologyFilter("Purchase invoice", "INVOICE")).toBe(true);
  });

  it("does not match a name that lacks the query", () => {
    expect(matchesOntologyFilter("Purchase invoice", "supplier")).toBe(false);
  });

  it("matches everything when the query is blank", () => {
    expect(matchesOntologyFilter("Purchase invoice", "")).toBe(true);
    expect(matchesOntologyFilter("Purchase invoice", "   ")).toBe(true);
  });

  it("ignores leading and trailing whitespace in the query", () => {
    expect(matchesOntologyFilter("Purchase invoice", "  invoice  ")).toBe(true);
  });
});
