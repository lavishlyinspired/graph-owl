/** SPARQL results, as a table and as a graph — Epic 41.
 *
 *  `00f-ui-architecture.md` puts the part that can be wrong in a way somebody
 *  would act on in a pure function. For a query workbench that is: whether
 *  these results *can* be drawn as a graph at all, and what the columns are.
 *
 *  **The graph view is offered only when it would be honest.** Rendering
 *  arbitrary solutions as nodes and edges invents structure the query never
 *  asked for, and a picture invents confidence a table does not.
 */

/** One solution: variable name to its rendered term. */
export type Solution = Readonly<Record<string, string>>;

/** Column order, taken from the solutions themselves.
 *
 *  SPARQL's `SELECT ?a ?b` order is not in the response, so it is recovered
 *  from first appearance rather than sorted alphabetically: an author who
 *  wrote `?name ?owner` expects to read them that way, and alphabetical order
 *  silently rewrites their query's shape.
 *
 *  Later solutions may bind variables the first did not — `OPTIONAL` does
 *  exactly that — so every solution is inspected, not just the first.
 */
export function columns(solutions: readonly Solution[]): string[] {
  const seen: string[] = [];
  for (const solution of solutions) {
    for (const name of Object.keys(solution)) {
      if (!seen.includes(name)) seen.push(name);
    }
  }
  return seen;
}

/** The three variable names a graph needs, in the spellings people write. */
const TRIPLE_SHAPES: readonly (readonly [string, string, string])[] = [
  ["s", "p", "o"],
  ["subject", "predicate", "object"],
  ["from", "relationship", "to"],
];

export interface GraphShape {
  readonly subject: string;
  readonly predicate: string;
  readonly object: string;
}

/** Can these solutions be drawn as a graph, and under which variables?
 *
 *  `null` when they cannot. **Offering the view anyway is the failure mode**:
 *  two arbitrary columns rendered as nodes and an edge assert a relationship
 *  the query never returned, and a reader believes a picture more readily than
 *  a table.
 *
 *  A shape matches only when **every** solution binds all three. One row short
 *  of a triple is a row that would silently vanish from the picture, and a
 *  graph missing rows the table shows is worse than no graph.
 */
export function graphShape(solutions: readonly Solution[]): GraphShape | null {
  if (solutions.length === 0) return null;

  for (const [subject, predicate, object] of TRIPLE_SHAPES) {
    const complete = solutions.every(
      (row) =>
        typeof row[subject] === "string" &&
        typeof row[predicate] === "string" &&
        typeof row[object] === "string",
    );
    if (complete) return { subject, predicate, object };
  }
  return null;
}

export interface ResultNode {
  readonly id: string;
  readonly label: string;
}

export interface ResultEdge {
  readonly from: string;
  readonly to: string;
  readonly label: string;
}

export interface ResultGraph {
  readonly nodes: readonly ResultNode[];
  readonly edges: readonly ResultEdge[];
}

/** The solutions as a graph, under a shape [`graphShape`] found.
 *
 *  Nodes are deduplicated — a subject appearing in ten solutions is one node
 *  with ten edges, not ten overlapping nodes — and edges are not, because two
 *  identical triples in a result set is something the reader should see rather
 *  than something this should hide.
 */
export function toGraph(solutions: readonly Solution[], shape: GraphShape): ResultGraph {
  const nodes = new Map<string, ResultNode>();
  const edges: ResultEdge[] = [];

  for (const row of solutions) {
    const from = row[shape.subject];
    const to = row[shape.object];
    const label = row[shape.predicate];
    if (from === undefined || to === undefined || label === undefined) continue;

    // Unconditional: setting the same key twice with the same value is the
    // same map. A `has` guard here read as deduplication and was doing
    // nothing — the `Map` is what deduplicates.
    nodes.set(from, { id: from, label: display(from) });
    nodes.set(to, { id: to, label: display(to) });
    edges.push({ from, to, label: display(label) });
  }

  return { nodes: [...nodes.values()], edges };
}

/** A term as a reader reads it.
 *
 *  The local part of an IRI, because a results graph full of full IRIs is a
 *  graph where every label is the same eighty characters and the last six
 *  carry the meaning. The full term stays the node's id — it is what a fix or
 *  a follow-up query is written against.
 */
export function display(term: string): string {
  const trimmed = term.replace(/^<|>$/g, "");
  const cut = Math.max(trimmed.lastIndexOf("#"), trimmed.lastIndexOf("/"));
  // Only the "ends in a separator" case needs guarding: with no separator at
  // all `cut` is -1 and `slice(0)` is already the whole string. A `cut === -1`
  // clause beside this one restated that and did nothing.
  if (cut === trimmed.length - 1) return trimmed;
  return trimmed.slice(cut + 1);
}

/** How the answer should be read, given what came back and what it cost. */
export interface Verdict {
  /** Worth saying out loud above the results. */
  readonly warnings: readonly string[];
  readonly canDraw: boolean;
}

/** What a reader needs told before they trust these results.
 *
 *  Truncation first, because it is the one that makes a *wrong* answer look
 *  like a complete one — and this project refuses that everywhere else.
 */
export function verdict(
  solutions: readonly Solution[],
  meta: { readonly truncated: boolean; readonly factsScanned: number; readonly plan: readonly string[] },
): Verdict {
  const warnings: string[] = [];
  if (meta.truncated) {
    warnings.push(
      "The budget cut this answer short. Rows are missing, and which ones is not knowable — narrow the query rather than trusting what came back.",
    );
  }
  // A full scan is not an error; it is a cost the author should be able to see.
  // Saying nothing lets an expensive query look identical to a cheap one.
  if (meta.plan.some((scan) => scan.trim() === "? ? ?")) {
    warnings.push(
      `No triple pattern could be bounded, so the whole graph was read (${meta.factsScanned} facts). Binding a predicate or a subject usually fixes this.`,
    );
  }
  return { warnings, canDraw: graphShape(solutions) !== null };
}
