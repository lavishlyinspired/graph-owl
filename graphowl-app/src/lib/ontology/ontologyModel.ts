/** The ontology graph, read from the same generic `/sparql` surface every
 *  other real screen in this console uses — no dedicated backend endpoint
 *  exists or is needed. `packs/gst/ontology.ttl`'s own comment explains why:
 *  every class and property it declares is a *runtime* term loaded through
 *  the same pack-loading path as any other data, sitting in the same triple
 *  store.
 *
 *  Ported in spirit, not in code, from `_archived/ui/src/features/
 *  ontology-builder/formats.ts`'s `importNTriples`/`triplesToModel` — that
 *  importer accepts five serialisations pasted or uploaded as text; this
 *  reads exactly one shape (the `{s,p,o}` rows `/sparql` already returns)
 *  and skips the text-serialise-then-reparse round trip that shape made
 *  necessary. It also drops several fields the archived model carried with
 *  no real data behind them — see the doc comment on `OntologyModel`. */

export interface OntologyClass {
  readonly id: string;
  readonly name: string;
  /** The IRI prefix this class was declared under (`https://graph-owl.dev/
   *  packs/gst#`, say) — every pack's own namespace, not a shared one. */
  readonly namespace: string;
}

export interface OntologyRelationship {
  readonly id: string;
  readonly label: string;
  readonly from: string;
  readonly to: string;
}

/** A plain `{prefix}:Property` declaration — a literal-valued attribute,
 *  not an `owl:ObjectProperty` between two classes. Listed on its own,
 *  deliberately never attributed to a class: see {@link OntologyModel}'s
 *  own doc comment for why that half of the picture stays a named gap. */
export interface OntologyProperty {
  readonly id: string;
  readonly name: string;
  readonly namespace: string;
}

/** **Deliberately narrower than the archived prototype's `OntologyModel`.**
 *  That shape also carried `Attribute`, `Cardinality`, `InteractionType`,
 *  `ReferenceDatum` and `SourceSystem` — none of which any real pack's
 *  ontology declares today, so they would sit permanently empty. A node's
 *  colour was stored per-class there too; here it is a rendering decision,
 *  resolved at draw time the same way `graphModel.ts` resolves colour for
 *  the instance graph, not persisted model data.
 *
 *  **Attribution to a class is the one gap kept in mind rather than
 *  modelled.** A plain `gst:Property` triple has no `rdfs:domain`, so
 *  *which class it belongs to* is not declared anywhere — only inferable
 *  from which class's *instances* actually carry it, a materially
 *  different (and bigger) query than reading a declaration. The property
 *  itself is real, declared data, though, so it is still listed — just
 *  flat, with no owning class, rather than invented or hidden. */
export interface OntologyModel {
  readonly classes: readonly OntologyClass[];
  readonly relationships: readonly OntologyRelationship[];
  readonly properties: readonly OntologyProperty[];
}

const OWL_OBJECT_PROPERTY = "http://www.w3.org/2002/07/owl#ObjectProperty";
const RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_DOMAIN = "http://www.w3.org/2000/01/rdf-schema#domain";
const RDFS_RANGE = "http://www.w3.org/2000/01/rdf-schema#range";

/** One query, scoped to one pack's own ontology graph.
 *
 *  The graph IRI is not a guess: `https://graph-owl.dev/ns/catalog#
 *  graph:import:{packId}-ontology` was confirmed by querying the running
 *  server directly (`SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s a gst:Class
 *  } }`) before this was written, not read off a comment in the archived
 *  importer and trusted. */
export function ontologyGraphQuery(packId: string): string {
  const graph = `https://graph-owl.dev/ns/catalog#graph:import:${packId}-ontology`;
  return `SELECT ?s ?p ?o WHERE { GRAPH <${graph}> { ?s ?p ?o } }`;
}

function stripBrackets(term: string): string {
  return term.startsWith("<") && term.endsWith(">") ? term.slice(1, -1) : term;
}

function literalValue(term: string): string {
  const match = /^"(.*)"(?:\^\^\S+|@[a-z-]+)?$/s.exec(term);
  return match ? match[1]! : term;
}

function localNameOf(iri: string): string {
  const cut = Math.max(iri.lastIndexOf("#"), iri.lastIndexOf("/"));
  return cut === -1 ? iri : iri.slice(cut + 1);
}

function namespaceOf(iri: string): string {
  const cut = Math.max(iri.lastIndexOf("#"), iri.lastIndexOf("/"));
  return cut === -1 ? iri : iri.slice(0, cut + 1);
}

/** Every pack declares its *own* `{prefix}:Class`/`{prefix}:label` under
 *  its own namespace rather than reusing a shared RDFS/OWL term —
 *  `packs/hospitality/ontology.ttl`'s own comment records that reusing
 *  `rdfs:label` directly was tried and refused, for the same reason a
 *  shared `owl:Class` is not what any pack actually declares its classes
 *  as. Recognising the *local name*, regardless of which pack's namespace
 *  it is under, is what makes this work for every pack rather than only
 *  the one it happened to be written against. */
function localNameIs(iri: string, name: string): boolean {
  return localNameOf(iri) === name;
}

export function ontologyModelFromSparqlRows(
  rows: readonly Record<string, string>[],
): OntologyModel {
  // `stripBrackets` is a safe no-op on a quoted literal (it only strips when
  // the term starts with `<` and ends with `>`), so the object term is
  // stripped unconditionally here — an IRI object needs it for the local-name
  // checks below, and a literal object passes through untouched for
  // `literalValue` to unwrap separately.
  const triples = rows.map((row) => ({
    subject: stripBrackets(row["s"] ?? ""),
    predicate: stripBrackets(row["p"] ?? ""),
    object: stripBrackets(row["o"] ?? ""),
  }));

  const classIris = new Set(
    triples
      .filter((t) => t.predicate === RDF_TYPE && localNameIs(t.object, "Class"))
      .map((t) => t.subject),
  );

  const labelOf = new Map<string, string>();
  for (const t of triples) {
    if (localNameIs(t.predicate, "label")) labelOf.set(t.subject, literalValue(t.object));
  }

  const classes: OntologyClass[] = [...classIris].map((iri) => ({
    id: iri,
    name: labelOf.get(iri) ?? localNameOf(iri),
    namespace: namespaceOf(iri),
  }));

  const propertyIris = new Set(
    triples
      .filter((t) => t.predicate === RDF_TYPE && localNameIs(t.object, "Property"))
      .map((t) => t.subject),
  );

  const properties: OntologyProperty[] = [...propertyIris].map((iri) => ({
    id: iri,
    name: labelOf.get(iri) ?? localNameOf(iri),
    namespace: namespaceOf(iri),
  }));

  const relationshipIris = new Set(
    triples
      .filter((t) => t.predicate === RDF_TYPE && t.object === OWL_OBJECT_PROPERTY)
      .map((t) => t.subject),
  );

  const domainOf = new Map<string, string>();
  const rangeOf = new Map<string, string>();
  for (const t of triples) {
    if (t.predicate === RDFS_DOMAIN) domainOf.set(t.subject, t.object);
    if (t.predicate === RDFS_RANGE) rangeOf.set(t.subject, t.object);
  }

  const relationships: OntologyRelationship[] = [...relationshipIris]
    .map((iri) => ({
      id: iri,
      label: labelOf.get(iri) ?? localNameOf(iri),
      from: domainOf.get(iri),
      to: rangeOf.get(iri),
    }))
    // Both ends must be a class this same picture actually knows about —
    // the same rule `toG6Data` applies to the instance graph in
    // `graphModel.ts`: an edge to a node not present in the picture is
    // dropped rather than drawn to nowhere.
    .filter(
      (r): r is OntologyRelationship =>
        r.from !== undefined && r.to !== undefined && classIris.has(r.from) && classIris.has(r.to),
    );

  return { classes, relationships, properties };
}
