/** The loaded import sources, for the Explore "Pack data" block — Plan 115
 *  Slice B1.
 *
 *  **One query answers "what has actually been imported".** Every pack import
 *  lands in `graph:import:{source}` (see `importFile.ts`'s `importThroughSurface`
 *  and the server's `/graph/import/rdf` handler), so a single
 *  `SELECT ?g (COUNT(?s) AS ?n) ... GROUP BY ?g` over the whole graph lists
 *  every loaded source and how many triples it holds — the same source name a
 *  successful upload prints in its toast (C1), so the user can match the two.
 */

import type { Solution } from "../../workbench/results";
import { lexical } from "../../workbench/results";

/** One named import graph that actually holds data. */
export interface LoadedSource {
  /** The import source name, e.g. `gst-gstr2b-2025-07` — the same name the
   *  upload toast prints. */
  readonly name: string;
  /** The pack it belongs to, from the source-name prefix (`gst-...`). */
  readonly packId: string;
  /** The graph IRI **as the wire reported it** (`https://graph-owl.dev/ns/
   *  catalog#graph:import:{name}`). Kept so the source view can query that
   *  exact graph by what the graph said, rather than re-assembling the IRI
   *  from the name and a hardcoded catalog prefix. */
  readonly iri: string;
  /** Triples the named graph holds. */
  readonly triples: number;
}

/** Every named graph and how much it holds. One query answers "what has
 *  actually been imported" — shared by every reader of the named-graph
 *  listing ([`PackDataExplorer`], the ontology editor's pack loader) so the
 *  query is written once. */
export const NAMED_GRAPHS_QUERY =
  "SELECT ?g (COUNT(?s) AS ?n) WHERE { GRAPH ?g { ?s ?p ?o } } GROUP BY ?g";

const IMPORT_GRAPH_PREFIX = "graph:import:";

/** `graph:import:gst-gstr2b-2025-07` → `gst-gstr2b-2025-07`. Anything that is
 *  not an import graph is `null` — a vocabulary graph is a real graph and
 *  must not be offered as "data you imported".
 *
 *  **The graph arrives as its full IRI, not its internal address.** A `Sid`
 *  is stored as `dsc:{id}` and `/sparql` renders one as
 *  `https://graph-owl.dev/ns/catalog#{id}` (see `graph-owl-core/src/flake.rs`),
 *  so the source name has to be read out of the IRI *fragment* — the part
 *  after the final `#` — not matched against the bare `graph:import:` prefix
 *  the internal form uses. Deriving the fragment instead of hardcoding the
 *  catalog IRI means the same function works whichever namespace a graph
 *  address eventually lives under. */
export function importSourceOf(graphIri: string): string | null {
  const local = graphIri.includes("#") ? graphIri.slice(graphIri.lastIndexOf("#") + 1) : graphIri;
  return local.startsWith(IMPORT_GRAPH_PREFIX) && local.length > IMPORT_GRAPH_PREFIX.length
    ? local.slice(IMPORT_GRAPH_PREFIX.length)
    : null;
}

/** The pack id a source name belongs to — the part before the first `-`,
 *  which is exactly how `importFile.ts` composes the name (`{pack}-{key}-{period}`).
 *  A name with no `-` keeps itself, so a malformed graph never crashes the
 *  listing. */
function packIdOfName(source: string): string {
  const dash = source.indexOf("-");
  return dash <= 0 ? source : source.slice(0, dash);
}

/** The loaded import sources out of one named-graph listing.
 *
 *  Values pass through `lexical`, because `POST /sparql` returns N-Triples:
 *  a graph name arrives as `<graph:import:…>` and a count as
 *  `"42"^^<…integer>`, and both must be unwrapped before they mean anything. */
export function loadedSourcesFromSparql(rows: readonly Solution[]): readonly LoadedSource[] {
  const out: LoadedSource[] = [];
  for (const row of rows) {
    const iri = lexical(row.g ?? "");
    const name = importSourceOf(iri);
    if (name === null) continue;
    out.push({ name, packId: packIdOfName(name), iri, triples: Number(lexical(row.n ?? "") || 0) });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** The readable tail of a subject IRI — `https://graph-owl.dev/packs/gst#2b-INV-1010`
 *  → `2b-INV-1010`. What a listing row is called; a full IRI in a list is
 *  noise, but the local name alone is the identifier the graph itself uses. */
export function localNameOf(iri: string): string {
  const hash = iri.lastIndexOf("#");
  return hash >= 0 ? iri.slice(hash + 1) : iri;
}

/** One subject a source's graph asserts about, with enough to read the row:
 *  what it is (`kind`, from `rdf:type`) and how much the graph says about it. */
export interface SourceSubject {
  /** The full subject IRI — the seed a neighbourhood walk (`SubjectExplorer`)
   *  is called with. */
  readonly iri: string;
  readonly localName: string;
  /** The `rdf:type`'s local name, or `null` when the source asserts no type. */
  readonly kind: string | null;
  readonly triples: number;
}

/** The subject listing of one source's graph: every subject it asserts, with
 *  how much is asserted about it. Scoped to the graph *the source reports*
 *  (`source.iri`) — the view asks the graph itself what it holds. */
export function subjectsQuery(graphIri: string): string {
  return `SELECT ?s (COUNT(?p) AS ?n) WHERE { GRAPH <${graphIri}> { ?s ?p ?o } } GROUP BY ?s`;
}

/** The `rdf:type` map of the same graph, joined by `subjectsFromSparql`. */
export function typesQuery(graphIri: string): string {
  return `SELECT ?s ?t WHERE { GRAPH <${graphIri}> { ?s a ?t } }`;
}

/** Every triple a source's graph asserts, unscoped by subject — Plan 116
 *  Slice A. Where [`subjectsQuery`]/[`typesQuery`] summarise a source for a
 *  listing, this reads it whole: `?s ?p ?o` rows feed
 *  [`ntriplesFromRows`], which formats them back into a document the
 *  ontology editor's existing N-Triples parser already reads. `CONSTRUCT`
 *  does not execute against this engine (verified live), so `SELECT` plus
 *  client-side formatting is the whole-graph read, not a shortcut around a
 *  missing one. */
export function triplesQuery(graphIri: string): string {
  return `SELECT ?s ?p ?o WHERE { GRAPH <${graphIri}> { ?s ?p ?o } }`;
}

/** The subjects of one source's graph, merged from its two wire answers:
 *  a subject/count listing and a subject/type map. Both are already scoped to
 *  the source's graph by the queries that produced them (`GRAPH <iri>`); this
 *  function only joins the two and names things.
 *
 *  Values pass through `lexical` for the same reason the source listing does
 *  — `/sparql` returns N-Triples terms, and a subject arrives as `<…>` while
 *  a count arrives as `"42"^^<…integer>`. */
export function subjectsFromSparql(
  subjectRows: readonly Solution[],
  typeRows: readonly Solution[],
): readonly SourceSubject[] {
  const kindByIri = new Map<string, string>();
  for (const row of typeRows) {
    const iri = lexical(row.s ?? "");
    const kind = lexical(row.t ?? "");
    if (iri === "" || kind === "") continue;
    if (!kindByIri.has(iri)) kindByIri.set(iri, kind);
  }

  const out: SourceSubject[] = [];
  for (const row of subjectRows) {
    const iri = lexical(row.s ?? "");
    if (iri === "") continue;
    const kind = kindByIri.get(iri);
    out.push({
      iri,
      localName: localNameOf(iri),
      kind: kind === undefined ? null : localNameOf(kind),
      triples: Number(lexical(row.n ?? "") || 0),
    });
  }
  return out.sort((a, b) => a.localName.localeCompare(b.localName));
}

/** A source's own triples, formatted as N-Triples text — Plan 116 Slice A.
 *
 *  **Turns a plain `SELECT ?s ?p ?o` result back into a document the
 *  ontology editor already knows how to parse**, so previewing an installed
 *  pack's already-loaded ontology as one connected graph needs no new
 *  parser and no new backend endpoint: `CONSTRUCT` does not execute against
 *  this engine (verified live — it returns no rows where the equivalent
 *  `SELECT` returns every triple), but `/sparql`'s `SELECT` rows already
 *  arrive with `s`/`p`/`o` as literal N-Triples terms — `<iri>`, `"literal"`,
 *  `"literal"^^<datatype>` — the same shape `packData.ts`'s other functions
 *  unwrap with `lexical()`. This one does not unwrap them; it joins them
 *  as-is, because the target is a document, not a value. */
export function ntriplesFromRows(rows: readonly Solution[]): string {
  const lines: string[] = [];
  for (const row of rows) {
    const s = row.s;
    const p = row.p;
    const o = row.o;
    if (s === undefined || p === undefined || o === undefined) continue;
    lines.push(`${s} ${p} ${o} .`);
  }
  return lines.join("\n");
}

/** A pack's own ontology source, if it has been loaded — Plan 116 Slice A.
 *
 *  **`{packId}-ontology` is not a guess; it is the convention every shipped
 *  pack's own manifest already follows.** Both `packs/gst/pack.toml` and
 *  `packs/hospitality/pack.toml` declare `path = "ontology.ttl"` with
 *  `source = "{packId}-ontology"` — the same `{pack}-{key}` shape
 *  `importFile.ts` uses for every other document, with `ontology` as the
 *  key. Matching by that name, scoped to the pack's own sources
 *  ([`sourcesForPack`]) rather than by name alone, means one pack's
 *  ontology source can never be handed back for a different pack. `null`
 *  for a pack whose ontology has not been loaded (or has no ontology
 *  document at all) — a real, expected state, not an error. */
export function ontologySourceFor(
  packId: string,
  sources: readonly LoadedSource[],
): LoadedSource | null {
  const name = `${packId}-ontology`;
  return sourcesForPack(sources, packId).find((source) => source.name === name) ?? null;
}

/** The loaded sources that belong to one installed pack. Matching on the
 *  source-name prefix keeps connector imports (`connector:erpnext` brings
 *  data, not a pack) out of a pack's listing without any of them disappearing
 *  from the graph. */
export function sourcesForPack(
  sources: readonly LoadedSource[],
  packId: string,
): readonly LoadedSource[] {
  return sources.filter((source) => source.packId === packId);
}
