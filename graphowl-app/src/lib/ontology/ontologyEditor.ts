/** Pure logic behind the Ontology Editor panel — `plans/ontology-editor.md`.
 *
 *  Deliberately thin: parsing, shape validation and reasoning all happen
 *  server-side (`/ontology-editor/{preview,dry-run,save}`, already shipped
 *  for Epic 42 Slice G). What lives here is the part worth unit-testing —
 *  the fixed graph this editor reads from, and turning its typed results
 *  into the one-line summaries the panel displays. */

import type { OntologyDryRunResult, OntologySaveResult } from "../api";

/** `Catalog::import_graph("ontology-editor")` — verified against
 *  `graph-owl-api/src/lib.rs` (`Sid::dsc(format!("graph:import:{source}"))`),
 *  not derived from a pack id. This is a single, fixed graph — every save
 *  through `/ontology-editor/save` retracts and re-lands here, regardless
 *  of which pack (if any) is selected elsewhere in the Ontology tab. */
export function ontologyEditorGraphQuery(): string {
  return "SELECT ?s ?p ?o WHERE { GRAPH <https://graph-owl.dev/ns/catalog#graph:import:ontology-editor> { ?s ?p ?o } }";
}

function pluralize(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function formatOntologyCheckSummary(result: OntologyDryRunResult): string {
  if (result.kind === "syntaxError") return `Syntax error: ${result.message}`;
  const accepted = result.accepted.length > 0 ? result.accepted.join(", ") : "nothing";
  return `Would accept: ${accepted} (${pluralize(result.newInferences, "new inference")})`;
}

export function formatOntologySaveSummary(result: OntologySaveResult): string {
  if (result.kind === "syntaxError") return `Could not save: ${result.message}`;
  return `Saved: ${pluralize(result.landed.length, "subject")} landed.`;
}
