/** Real, copyable SPARQL for one entity's own Queries tab — reuses the
 *  `GRAPH ?g { ... }` shape this whole console had to learn the hard way
 *  (facts live in named import graphs, never the default one). */

export function outgoingFactsQuery(iri: string): string {
  return `SELECT ?p ?o WHERE { GRAPH ?g { <${iri}> ?p ?o } }`;
}

export function incomingReferencesQuery(iri: string): string {
  return `SELECT ?s ?p WHERE { GRAPH ?g { ?s ?p <${iri}> } }`;
}
