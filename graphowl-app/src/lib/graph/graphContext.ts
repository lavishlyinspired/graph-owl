/** Mapping `POST /graph/context`'s response onto the console's own
 *  `GraphView` shape.
 *
 *  `/graph/context` (`graph_owl_server::graph_context_route`) walks **any**
 *  subject, not only a catalog asset — Explore's seed picker links out to
 *  findings whose subject is a bare graph IRI (a GST invoice, say), which has
 *  no row in `assets` and therefore no UUID `/assets/{id}/graph` could ever
 *  accept. This is the endpoint that already exists for exactly that case;
 *  it was simply never wired into this console. */

import type { GraphEdge, GraphNode, GraphView } from "../api";

export interface RawGraphContextNode {
  readonly id: string;
  readonly iri: string | null;
  readonly label: string | null;
  /** The named import graphs this subject appears in. */
  readonly sources?: readonly string[];
}

export interface RawGraphContextEdge {
  readonly from: string;
  readonly to: string;
  readonly relationship: string;
  readonly derived?: boolean;
}

export interface RawGraphContext {
  readonly nodes: readonly RawGraphContextNode[];
  readonly edges: readonly RawGraphContextEdge[];
  readonly truncated: boolean;
}

/** `/graph/context` keys both `nodes[].id` and `edges[].from`/`to` by each
 *  subject's short local name — fine as a response's own internal key, but a
 *  follow-up expansion sends that id straight back as the next seed, and the
 *  server's `parse_node_id` only accepts a UUID, a `namespace:local` pair, or
 *  a full IRI. Every id in the returned picture is rewritten to the node's
 *  own IRI so the picture stays expandable; a node whose namespace has no
 *  known IRI prefix keeps its short id rather than losing it. */
export function toGraphView(raw: RawGraphContext): GraphView {
  const longId = new Map(raw.nodes.map((node) => [node.id, node.iri ?? node.id]));
  const resolve = (shortId: string) => longId.get(shortId) ?? shortId;

  const nodes: GraphNode[] = raw.nodes.map((node) => ({
    id: resolve(node.id),
    name: node.label ?? node.id,
    kind: null,
    ...(node.sources && node.sources.length > 0 ? { sources: node.sources } : {}),
  }));

  const edges: GraphEdge[] = raw.edges.map((edge) => ({
    from: resolve(edge.from),
    to: resolve(edge.to),
    relationship: edge.relationship,
    derived: edge.derived,
  }));

  return { nodes, edges, truncated: raw.truncated };
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Which of the two neighbourhood endpoints a seed needs — matches the
 *  server's own `parse_node_id`: a bare UUID is a catalog asset id
 *  (`/assets/{id}/graph`); an IRI or `namespace:local` identifier is a
 *  graph-only subject (`/graph/context`). */
export function looksLikeAssetId(id: string): boolean {
  return UUID_RE.test(id);
}

/** One `?s ?t` row as `/sparql` returns it — IRIs arrive angle-bracketed. */
export type TypeRow = Readonly<Record<string, string>>;

function stripBrackets(term: string): string {
  return term.startsWith("<") && term.endsWith(">") ? term.slice(1, -1) : term;
}

/** `https://graph-owl.dev/packs/gst#Supplier` → `gst:Supplier`.
 *
 *  **Derived from the IRI's own shape, never from a table of known types.**
 *  The console cannot ship a mapping of business classes without becoming a
 *  GST console; the local name is whatever follows the final `#` or `/`, and
 *  the prefix is the path segment that precedes it — which is the vocabulary's
 *  own name for any pack laid out this way. An IRI with no such segment keeps
 *  the bare local name rather than inventing a prefix for it. */
function shortenTypeIri(iri: string): string {
  const hash = iri.lastIndexOf("#");
  const base = hash === -1 ? iri.slice(0, iri.lastIndexOf("/")) : iri.slice(0, hash);
  const local = hash === -1 ? iri.slice(iri.lastIndexOf("/") + 1) : iri.slice(hash + 1);

  // The prefix must be a *path* segment. Dropping the scheme first is what
  // distinguishes `.../packs/gst#Supplier` (prefix `gst`) from
  // `https://ex.org/Thing`, whose final segment before the local name is the
  // host — and `ex.org:Thing` would read as a vocabulary prefix that does not
  // exist.
  const authorityAndPath = base.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "");
  if (!authorityAndPath.includes("/")) return local;

  const prefix = authorityAndPath.slice(authorityAndPath.lastIndexOf("/") + 1);
  return prefix && prefix !== local ? `${prefix}:${local}` : local;
}

/** One query for every node's `rdf:type`, or `null` when there is nothing to
 *  ask about — an empty `VALUES` block is a query that can only return zero
 *  rows, and paying a round trip for it is waste.
 *
 *  **`GRAPH ?g` is not optional.** Imports live in named graphs; a bare
 *  `?s a ?t` matches the default graph only and returns zero rows, which is
 *  indistinguishable from "nothing here is typed" at the call site. */
export function nodeTypeQuery(iris: readonly string[]): string | null {
  if (iris.length === 0) return null;
  const values = iris.map((iri) => `<${iri}>`).join(" ");
  return `SELECT ?s ?t WHERE { GRAPH ?g { ?s a ?t } VALUES ?s { ${values} } }`;
}

/** Subject IRI → its short type label.
 *
 *  A subject is typed once per named graph it appears in, so identical rows
 *  repeat; and a subject may genuinely carry more than one class. Both
 *  collapse to a single label, chosen by sort order so the same data resolves
 *  the same way on every load — an unstable pick would recolour a node and
 *  move its legend entry between refreshes. */
export function typesFromTypeRows(rows: readonly TypeRow[]): ReadonlyMap<string, string> {
  const bySubject = new Map<string, string[]>();
  for (const row of rows) {
    const subject = row["s"];
    const type = row["t"];
    if (subject === undefined || type === undefined) continue;
    const key = stripBrackets(subject);
    const label = shortenTypeIri(stripBrackets(type));
    const seen = bySubject.get(key);
    if (seen) {
      if (!seen.includes(label)) seen.push(label);
    } else {
      bySubject.set(key, [label]);
    }
  }
  return new Map([...bySubject].map(([key, labels]) => [key, [...labels].sort()[0]!]));
}

/** The same picture with each node's resolved type attached.
 *
 *  This is what makes the canvas legible: `semanticType` is what
 *  `graphModel`'s `nodeColor`, `nodeClasses` and `legendEntries` all key off,
 *  so a node without one draws in the muted `hidden-kind` treatment and
 *  contributes nothing to the legend. A node the graph could not type keeps
 *  no type at all rather than being given a placeholder one. */
export function withNodeTypes(
  view: GraphView,
  types: ReadonlyMap<string, string>,
): GraphView {
  return {
    ...view,
    nodes: view.nodes.map((node) => {
      const type = types.get(node.id);
      return type === undefined ? node : { ...node, semanticType: type };
    }),
  };
}

/** Where a node's "open" action leads.
 *
 *  **Explore's own Entity tab, for either id shape.** This used to be a
 *  separate `/entity/:id` page; that page's content is now `EntityPanel`,
 *  embedded in Explore itself so the graph and the entity detail share one
 *  picker instead of being two screens a reader has to hop between.
 *  `?view=entity` is what actually lands on the detail view rather than
 *  Explore's own default graph canvas — `/entity/:id` still exists as a
 *  redirect to this same URL, for anything (a saved link, a test) that
 *  still constructs the old path directly. */
export function openTargetFor(id: string): string {
  return `/explore/${encodeURIComponent(id)}?view=entity`;
}
