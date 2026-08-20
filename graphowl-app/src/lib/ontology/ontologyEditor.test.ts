import { describe, expect, it } from "vitest";
import {
  formatOntologyCheckSummary,
  formatOntologySaveSummary,
  ontologyEditorGraphQuery,
} from "./ontologyEditor";
import type { OntologyDryRunResult, OntologySaveResult } from "../api";

describe("the ontology editor's own fixed graph", () => {
  /** `Catalog::import_graph("ontology-editor")` — verified against the
   *  server source (`graph-owl-api/src/lib.rs`), not guessed: `Sid::dsc`
   *  prefixes with the catalog namespace, same convention this session's
   *  `ontologyGraphQuery` already confirmed empirically for pack graphs. */
  it("queries the fixed editor graph, not any pack's own", () => {
    expect(ontologyEditorGraphQuery()).toBe(
      "SELECT ?s ?p ?o WHERE { GRAPH <https://graph-owl.dev/ns/catalog#graph:import:ontology-editor> { ?s ?p ?o } }",
    );
  });
});

describe("summarising a Check (dry-run) result", () => {
  it("reports a syntax error plainly", () => {
    const result: OntologyDryRunResult = { kind: "syntaxError", message: "unexpected token", line: 3, column: 5 };
    expect(formatOntologyCheckSummary(result)).toBe("Syntax error: unexpected token");
  });

  it("names what was accepted and the new-inference count", () => {
    const result: OntologyDryRunResult = {
      kind: "checked",
      accepted: ["ex:Widget", "ex:Gadget"],
      rejected: [],
      newInferences: 3,
    };
    expect(formatOntologyCheckSummary(result)).toBe("Would accept: ex:Widget, ex:Gadget (3 new inferences)");
  });

  it("singularises one new inference", () => {
    const result: OntologyDryRunResult = { kind: "checked", accepted: ["ex:Widget"], rejected: [], newInferences: 1 };
    expect(formatOntologyCheckSummary(result)).toBe("Would accept: ex:Widget (1 new inference)");
  });

  it("says so when nothing would be accepted", () => {
    const result: OntologyDryRunResult = { kind: "checked", accepted: [], rejected: [], newInferences: 0 };
    expect(formatOntologyCheckSummary(result)).toBe("Would accept: nothing (0 new inferences)");
  });
});

describe("summarising a Save result", () => {
  it("reports a syntax error plainly, distinct from Check's wording", () => {
    const result: OntologySaveResult = { kind: "syntaxError", message: "unterminated literal", line: 1, column: 9 };
    expect(formatOntologySaveSummary(result)).toBe("Could not save: unterminated literal");
  });

  it("counts landed subjects", () => {
    const result: OntologySaveResult = { kind: "saved", landed: ["ex:Widget", "ex:Gadget"], skipped: [], rejected: [] };
    expect(formatOntologySaveSummary(result)).toBe("Saved: 2 subjects landed.");
  });

  it("singularises one landed subject", () => {
    const result: OntologySaveResult = { kind: "saved", landed: ["ex:Widget"], skipped: [], rejected: [] };
    expect(formatOntologySaveSummary(result)).toBe("Saved: 1 subject landed.");
  });
});
