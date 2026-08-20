/** Every real instance a pack's own graph declares — the Explore entity
 *  picker's "all entities" list, not only the ones a rule has flagged.
 *
 *  Read through the same generic `/sparql` surface every other real
 *  screen in this console uses. Scoped by namespace, not by named graph:
 *  unlike the ontology's own graph (`graph:import:{packId}-ontology`),
 *  instance data lands in per-import graphs named after the source run
 *  (`graph:import:reco-*-books`, `...-gstr2b`, ...), so there is no single
 *  graph name to scope to — the pack's own IRI namespace is the only
 *  thing every one of a pack's instances shares. */

export interface EntitySummary {
  readonly id: string;
  readonly iri: string;
  readonly type: string;
}

export function allEntitiesQuery(packId: string): string {
  return `SELECT ?s ?type WHERE { GRAPH ?g { ?s a ?type } FILTER(CONTAINS(STR(?type), "/packs/${packId}#")) }`;
}

function stripBrackets(term: string): string {
  return term.startsWith("<") && term.endsWith(">") ? term.slice(1, -1) : term;
}

function localNameOf(iri: string): string {
  const cut = Math.max(iri.lastIndexOf("#"), iri.lastIndexOf("/"));
  return cut === -1 ? iri : iri.slice(cut + 1);
}

/** `a gst:Class`/`a gst:Property` is the ontology's own TBox declaration
 *  — the same suffix-recognition `ontologyModel.ts` uses to find classes
 *  in the first place, applied here to keep them out of an instance
 *  picker instead. */
const SCHEMA_TYPES = new Set(["Class", "Property"]);

export function entitiesFromSparqlRows(rows: readonly Record<string, string>[]): EntitySummary[] {
  const bySubject = new Map<string, EntitySummary>();
  for (const row of rows) {
    const iri = stripBrackets(row["s"] ?? "");
    const type = localNameOf(stripBrackets(row["type"] ?? ""));
    if (SCHEMA_TYPES.has(type) || bySubject.has(iri)) continue;
    bySubject.set(iri, { id: localNameOf(iri), iri, type });
  }
  return [...bySubject.values()].sort((a, b) => a.type.localeCompare(b.type) || a.id.localeCompare(b.id));
}
