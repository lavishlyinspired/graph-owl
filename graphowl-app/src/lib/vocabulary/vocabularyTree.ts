/** Turning a flat term list plus SKOS relations into a poly-hierarchy tree —
 *  Epic 42 Slice A.
 *
 *  **Why this cannot reuse `admin/hierarchy.ts`'s shape.** A team has one
 *  `parentTeamId`; SKOS explicitly permits a term several `broader` parents
 *  (`plans/42-ui-semantic-surfaces.md`, decision — "a poly-hierarchy term
 *  ... renders under **each** parent without duplicating identity"). A tree
 *  keyed by render position (one node object per occurrence) is what makes
 *  that possible; `termId` is the identity a caller must compare selection,
 *  not `renderKey`, which exists only so a tree widget has a unique key per
 *  occurrence.
 *
 *  **Only `broader` is read, never `narrower`.** `GET
 *  /glossary-terms/{id}/relations` returns "every relation visible on this
 *  term, derived inverses included" — a term declaring `broader: parent`
 *  makes the *parent's* relations list carry a derived `narrower: term`
 *  automatically. Reading only `broader` and inverting it here means this
 *  function does not depend on which direction a caller's fetch happened to
 *  populate, and a fixture only needs to declare one direction.
 *
 *  **`build` always returns a node, never `null`.** Every id it is ever
 *  called with — a term itself, or a child pulled from `childrenOf` — comes
 *  from `terms` or from a target `broaderTargetsOf` already checked against
 *  `byId`, so a lookup miss inside `build` cannot happen. An earlier version
 *  looked the term up by id again inside `build` and defended a `undefined`
 *  case that could not occur; mutation testing confirmed it unreachable, so
 *  it is threaded through as a `Term` instead of re-looked-up by id.
 */

export interface Term {
  readonly id: string;
  readonly name: string;
}

export interface Relation {
  readonly kind: "broader" | "narrower" | "related" | "exactMatch" | "closeMatch";
  readonly target: string;
}

export interface VocabularyTreeNode {
  /** Unique per rendered position (includes the path from the root) —
   *  needed only by the tree widget's own `key` prop. */
  readonly renderKey: string;
  /** The term's own identity, shared across every position it renders at.
   *  Selection state and the detail pane must key off this. */
  readonly termId: string;
  readonly term: Term;
  readonly children: readonly VocabularyTreeNode[];
  readonly depth: number;
  /** True when placing this node here would revisit an ancestor of its own
   *  path. Rendered as a marked, childless leaf rather than recursed into —
   *  a cycle is real data (a mistake upstream, or two terms declared
   *  broader of each other), not a reason to hang. */
  readonly isCyclic: boolean;
}

export interface VocabularyTree {
  readonly roots: readonly VocabularyTreeNode[];
}

export function buildVocabularyTree(
  terms: readonly Term[],
  relationsByTerm: ReadonlyMap<string, readonly Relation[]>,
): VocabularyTree {
  const byId = new Map(terms.map((t) => [t.id, t]));

  const broaderTargetsOf = (termId: string): string[] =>
    (relationsByTerm.get(termId) ?? [])
      .filter((r) => r.kind === "broader")
      // A parent this vocabulary does not contain cannot be rendered, so it
      // is treated the same as having no parent at all — not as a reason to
      // drop the term.
      .map((r) => r.target)
      .filter((target) => byId.has(target));

  // Parent id → child terms, inverted from every term's own `broader` list
  // — built once so `build` below is a lookup, not a re-scan per node.
  const childrenOf = new Map<string, Term[]>();
  for (const t of terms) {
    for (const parentId of broaderTargetsOf(t.id)) {
      const siblings = childrenOf.get(parentId);
      if (siblings) siblings.push(t);
      else childrenOf.set(parentId, [t]);
    }
  }

  const build = (
    term: Term,
    depth: number,
    ancestry: readonly string[],
    path: string,
  ): VocabularyTreeNode => {
    if (ancestry.includes(term.id)) {
      return { renderKey: path, termId: term.id, term, children: [], depth, isCyclic: true };
    }

    const nextAncestry = [...ancestry, term.id];
    const children = (childrenOf.get(term.id) ?? []).map((child) =>
      build(child, depth + 1, nextAncestry, `${path}/${child.id}`),
    );

    return { renderKey: path, termId: term.id, term, children, depth, isCyclic: false };
  };

  const roots: VocabularyTreeNode[] = [];
  const placed = new Set<string>();

  const markPlaced = (node: VocabularyTreeNode): void => {
    placed.add(node.termId);
    node.children.forEach(markPlaced);
  };

  const addRoot = (term: Term): void => {
    const node = build(term, 0, [], term.id);
    roots.push(node);
    markPlaced(node);
  };

  for (const t of terms) {
    if (broaderTargetsOf(t.id).length === 0) addRoot(t);
  }

  // A term whose only "parent" path loops back on itself has no true root
  // to be placed under, but it is still real data — not a reason to drop
  // it. One entry point per unreached component is enough: `addRoot`
  // marks everything its own subtree reaches, so a second term in the same
  // cycle is already `placed` by the time this loop gets to it and is not
  // re-added as a redundant second view of the identical cycle.
  for (const t of terms) {
    if (!placed.has(t.id)) addRoot(t);
  }

  return { roots };
}
