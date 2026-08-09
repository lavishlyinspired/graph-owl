/** Epic 42 Slice G: the text-first ontology editor's pure logic. The
 *  editor's text is the source; everything here turns a parsed document
 *  into what the graph pane draws, never the other way round.
 *
 *  **This module's own named RED test**: "a syntax error must keep the
 *  previous graph and mark the line" — `applyParseOutcome` must never
 *  clear `lastGood` on a `syntaxError` outcome. Blanking the picture on
 *  every half-typed triple makes the feedback the whole slice exists to
 *  give useless. */

import { canvasLabel } from "../../graph/bidiLabel";

export interface PreviewTriple {
  readonly s: string;
  readonly p: string;
  readonly o: string;
  readonly oIsRef: boolean;
}

export interface RdfEditPreview {
  readonly triples: readonly PreviewTriple[];
  readonly declared: readonly string[];
}

export interface SyntaxErrorInfo {
  readonly message: string;
  readonly line: number | null;
  readonly column: number | null;
}

export type ParseOutcome =
  | ({ readonly kind: "syntaxError" } & SyntaxErrorInfo)
  | { readonly kind: "preview"; readonly preview: RdfEditPreview };

export type EditorFormat = "turtle" | "ntriples" | "jsonld";

export interface EditorState {
  readonly document: string;
  readonly format: EditorFormat;
  /** The most recent successfully-parsed graph. Never cleared by a
   *  `syntaxError` outcome — only ever replaced by a later `preview`. */
  readonly lastGood: RdfEditPreview | null;
  readonly error: SyntaxErrorInfo | null;
}

export function initialEditorState(format: EditorFormat = "turtle"): EditorState {
  return { document: "", format, lastGood: null, error: null };
}

export function applyParseOutcome(
  state: EditorState,
  document: string,
  outcome: ParseOutcome,
): EditorState {
  if (outcome.kind === "syntaxError") {
    return {
      ...state,
      document,
      error: { message: outcome.message, line: outcome.line, column: outcome.column },
    };
  }
  return { ...state, document, error: null, lastGood: outcome.preview };
}

/** Everything up to and including the last `#`, or the last `/` when
 *  there is no `#`. An IRI with neither is returned as-is — it has no
 *  namespace to split off, not an empty one. */
export function namespaceOf(iri: string): string {
  const hash = iri.lastIndexOf("#");
  if (hash !== -1) return iri.slice(0, hash + 1);
  const slash = iri.lastIndexOf("/");
  return slash !== -1 ? iri.slice(0, slash + 1) : iri;
}

export function localName(iri: string): string {
  const ns = namespaceOf(iri);
  return ns.length < iri.length ? iri.slice(ns.length) : iri;
}

/** Every namespace a triple's subject, predicate, or ref object names —
 *  never a literal object's own text, which is not an IRI at all. */
export function namespacesIn(preview: RdfEditPreview): string[] {
  const namespaces = new Set<string>();
  for (const triple of preview.triples) {
    namespaces.add(namespaceOf(triple.s));
    namespaces.add(namespaceOf(triple.p));
    if (triple.oIsRef) namespaces.add(namespaceOf(triple.o));
  }
  return [...namespaces].sort();
}

export function predicatesIn(preview: RdfEditPreview): string[] {
  return [...new Set(preview.triples.map((triple) => triple.p))].sort();
}

const SUBSUMPTION_PREDICATES = new Set([
  "http://www.w3.org/2000/01/rdf-schema#subClassOf",
  "http://www.w3.org/2000/01/rdf-schema#subPropertyOf",
]);

export function isSubsumptionPredicate(predicate: string): boolean {
  return SUBSUMPTION_PREDICATES.has(predicate);
}

export interface GraphFilter {
  readonly namespace: string | null;
  readonly predicate: string | null;
}

export function filterTriples(
  preview: RdfEditPreview,
  filter: GraphFilter,
): readonly PreviewTriple[] {
  return preview.triples.filter((triple) => {
    if (filter.predicate && triple.p !== filter.predicate) return false;
    if (
      filter.namespace &&
      namespaceOf(triple.s) !== filter.namespace &&
      namespaceOf(triple.p) !== filter.namespace
    ) {
      return false;
    }
    return true;
  });
}

/** A Cytoscape element — the same structural shape `graph/cytoscape.ts`
 *  already establishes for the Explorer, so the two graph panes in this
 *  app draw from one contract even though their data models differ (the
 *  Explorer walks an expandable neighbourhood; this renders a fixed,
 *  fully-parsed document). Typed structurally rather than sharing the
 *  Explorer's own `Element` type, since this one's `classes` vocabulary
 *  (`declared`/`referenced`/`subsumption`/`property`) means something
 *  different from the Explorer's (`expandable`/`derived`/`seed`). */
export interface OntologyElement {
  readonly group: "nodes" | "edges";
  readonly data: {
    readonly id: string;
    readonly label?: string;
    readonly source?: string;
    readonly target?: string;
  };
  readonly classes: string;
}

/** The graph pane's own named RED test: a declared term and a merely
 *  referenced one must be visually distinguishable, and a subsumption
 *  edge must read differently from an ordinary property edge — an author
 *  who cannot see which is which will "fix" somebody else's vocabulary,
 *  or mistake a plain relationship for a class hierarchy. */
export function toOntologyElements(
  preview: RdfEditPreview,
  filter: GraphFilter,
): OntologyElement[] {
  const triples = filterTriples(preview, filter);
  const declared = new Set(preview.declared);

  const nodeIds = new Set<string>();
  for (const triple of triples) {
    nodeIds.add(triple.s);
    if (triple.oIsRef) nodeIds.add(triple.o);
  }

  const nodes: OntologyElement[] = [...nodeIds].map((id) => ({
    group: "nodes" as const,
    // `canvasLabel`: this pane draws through Cytoscape onto a `<canvas>`
    // too, and a term's local name can carry right-to-left text just as
    // freely as an asset name can (`bidiLabel.ts`'s own doc comment).
    data: { id, label: canvasLabel(localName(id)) },
    classes: declared.has(id) ? "declared" : "referenced",
  }));

  // Only a ref object becomes an edge — a literal-valued triple (a
  // `dsc:name` string, for instance) is a property on the node, not a
  // second node and an edge to it.
  const edges: OntologyElement[] = triples
    .filter((triple) => triple.oIsRef)
    .map((triple, index) => ({
      group: "edges" as const,
      data: {
        id: `${triple.s}→${triple.o}→${triple.p}→${index}`,
        source: triple.s,
        target: triple.o,
        label: localName(triple.p),
      },
      classes: isSubsumptionPredicate(triple.p) ? "subsumption" : "property",
    }));

  return [...nodes, ...edges];
}
