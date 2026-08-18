import { describe, expect, it } from "vitest";
import { buildVocabularyTree, type Relation, type Term } from "./vocabularyTree";

function term(id: string, name = id): Term {
  return { id, name };
}

function relation(kind: Relation["kind"], target: string): Relation {
  return { kind, target };
}

/** Every relation any term in the fixture declares — `buildVocabularyTree`
 *  reads a term's own `broader`/`narrower` edges by id, matching the shape
 *  `GET /glossary-terms/{id}/relations` returns per term ("derived inverses
 *  included" — a `broader` assertion on the child is what this map is keyed
 *  from, not a separate `narrower` assertion on the parent). */
function relationsOf(entries: readonly (readonly [string, readonly Relation[]])[]): ReadonlyMap<string, readonly Relation[]> {
  return new Map(entries);
}

function shape(nodes: ReturnType<typeof buildVocabularyTree>["roots"]): string[] {
  const lines: string[] = [];
  const walk = (list: typeof nodes, depth: number) => {
    for (const node of list) {
      lines.push(`${"  ".repeat(depth)}${node.termId}${node.isCyclic ? " (cyclic)" : ""}`);
      walk(node.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return lines;
}

describe("buildVocabularyTree", () => {
  it("shows an empty vocabulary as an empty tree, not an error", () => {
    const tree = buildVocabularyTree([], relationsOf([]));
    expect(tree.roots).toEqual([]);
  });

  it("nests a term with no broader relation as a root, with its narrower children beneath it", () => {
    const terms = [term("customer"), term("individual-customer")];
    const relations = relationsOf([
      ["individual-customer", [relation("broader", "customer")]],
    ]);

    const tree = buildVocabularyTree(terms, relations);

    expect(shape(tree.roots)).toEqual(["customer", "  individual-customer"]);
    expect(tree.roots[0]?.depth).toBe(0);
    expect(tree.roots[0]?.children[0]?.depth).toBe(1);
  });

  it("does not treat a non-broader relation as a parent pointer", () => {
    // `related` is a genuine SKOS relation, but it is not hierarchy — a
    // term whose only relation is `related` must still surface as its own
    // root, and the related term must not become its child.
    const terms = [term("debit"), term("credit")];
    const relations = relationsOf([["debit", [relation("related", "credit")]]]);

    const tree = buildVocabularyTree(terms, relations);

    expect(shape(tree.roots).sort()).toEqual(["credit", "debit"]);
  });

  it("gives a parent both of its children, not only the last one processed", () => {
    // A regression this exact shape can hide: writing each parent's
    // children with `set` instead of appending to what is already there
    // silently keeps only the most recently seen child.
    const terms = [term("customer"), term("individual-customer"), term("corporate-customer")];
    const relations = relationsOf([
      ["individual-customer", [relation("broader", "customer")]],
      ["corporate-customer", [relation("broader", "customer")]],
    ]);

    const tree = buildVocabularyTree(terms, relations);

    expect(tree.roots).toHaveLength(1);
    expect(tree.roots[0]?.children.map((c) => c.termId).sort()).toEqual([
      "corporate-customer",
      "individual-customer",
    ]);
  });

  /** **The RED test the epic's own plan names.** SKOS explicitly permits a
   *  term to have several `broader` parents (`plans/42-ui-semantic-surfaces.md`
   *  Slice A) — this is normal data, not an edge case. A tree keyed by
   *  render position must place the term under *each* parent; a tree that
   *  instead keys nodes by path would still pass a single-parent test, so
   *  this fixture only makes sense with more than one. */
  it("renders a poly-hierarchy term under every declared parent, sharing one identity", () => {
    const terms = [term("revenue"), term("finance-domain"), term("reporting-domain")];
    const relations = relationsOf([
      ["revenue", [relation("broader", "finance-domain"), relation("broader", "reporting-domain")]],
    ]);

    const tree = buildVocabularyTree(terms, relations);

    expect(shape(tree.roots)).toEqual([
      "finance-domain",
      "  revenue",
      "reporting-domain",
      "  revenue",
    ]);
    // Two renders of the same term must not be two identities: selecting
    // one occurrence has to be recognisable as selecting the other, which
    // only holds if `termId` — not `renderKey` — is what a caller compares.
    const revenueOccurrences = tree.roots.flatMap((root) => root.children);
    expect(revenueOccurrences).toHaveLength(2);
    expect(new Set(revenueOccurrences.map((node) => node.termId))).toEqual(new Set(["revenue"]));
    expect(new Set(revenueOccurrences.map((node) => node.renderKey)).size).toBe(2);
  });

  /** A cycle in the data must render, marked, rather than hang the walk —
   *  the plan's own acceptance criterion. Mutator watch: an unguarded
   *  recursive walk hangs on this fixture; this test is what proves the
   *  guard exists at all, since a passing suite with no cycle fixture would
   *  never notice an infinite loop was even possible. */
  it("marks a cyclic term rather than recursing forever", () => {
    const terms = [term("a"), term("b")];
    const relations = relationsOf([
      ["a", [relation("broader", "b")]],
      ["b", [relation("broader", "a")]],
    ]);

    const tree = buildVocabularyTree(terms, relations);

    // Neither `a` nor `b` has a broader relation reaching outside the
    // cycle, so there is no independent anchor — one entry point renders
    // the whole loop rather than showing it twice from two starting
    // points. `a` is chosen because it is first in `terms`.
    expect(shape(tree.roots)).toEqual(["a", "  b", "    a (cyclic)"]);
    const cyclicNode = tree.roots[0]?.children[0]?.children[0];
    expect(cyclicNode?.isCyclic).toBe(true);
    expect(cyclicNode?.children).toEqual([]);
  });

  it("gives every term in a larger cycle exactly one place in the tree, not zero and not several", () => {
    // `a` -> `b` -> `c` -> `a`, none reachable from outside the loop.
    const terms = [term("a"), term("b"), term("c")];
    const relations = relationsOf([
      ["a", [relation("broader", "c")]],
      ["b", [relation("broader", "a")]],
      ["c", [relation("broader", "b")]],
    ]);

    const tree = buildVocabularyTree(terms, relations);

    const seen: string[] = [];
    const walk = (nodes: typeof tree.roots) => {
      for (const node of nodes) {
        seen.push(node.termId);
        walk(node.children);
      }
    };
    walk(tree.roots);
    expect(seen.sort()).toEqual(["a", "a", "b", "c"]);
    expect(seen.filter((id) => id === "a")).toHaveLength(2); // the entry point, plus the cyclic close
    expect(tree.roots).toHaveLength(1); // one entry point for the whole loop, not three
  });

  it("treats a broader target outside the vocabulary as no parent, not a dropped term", () => {
    const terms = [term("orphan")];
    const relations = relationsOf([["orphan", [relation("broader", "not-in-this-glossary")]]]);

    const tree = buildVocabularyTree(terms, relations);

    expect(shape(tree.roots)).toEqual(["orphan"]);
  });

  it("places a term whose broader target is outside the vocabulary among the true roots, in input order", () => {
    // Distinguishes "no parent within the vocabulary" from "not yet placed,
    // fixed up on a second pass": if the out-of-vocabulary target were kept
    // rather than filtered, `orphan` would fail the same-pass root check
    // (a non-empty, if unusable, broader list) and only be added by the
    // cycle-fallback pass afterwards — landing *after* `finance`, not
    // before it, even though `orphan` comes first in `terms`.
    const terms = [term("orphan"), term("finance")];
    const relations = relationsOf([["orphan", [relation("broader", "not-in-this-glossary")]]]);

    const tree = buildVocabularyTree(terms, relations);

    expect(tree.roots.map((r) => r.termId)).toEqual(["orphan", "finance"]);
  });

  it("drops a narrower relation naming a term that does not exist, rather than rendering a phantom node", () => {
    const terms = [term("customer")];
    const relations = relationsOf([["customer", [relation("narrower", "does-not-exist")]]]);

    const tree = buildVocabularyTree(terms, relations);

    expect(shape(tree.roots)).toEqual(["customer"]);
  });
});
