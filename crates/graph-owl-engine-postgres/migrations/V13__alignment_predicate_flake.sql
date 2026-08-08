-- Phase 3 item 3.14: `alignmentPredicate` on the reified alignment node —
-- found missing while building Epic 42's alignment review queue.
-- `Alignment::subject()` already encodes the predicate in its own compound
-- local name (`alignment:{left}:{predicate}:{right}`), but nothing wrote it
-- as its own readable flake, so a review-queue reader with
-- left/right/source/confidence in hand had no way to reconstruct which
-- `skos:*Match` a "confirm" action should resubmit. Same registration
-- pattern V12 already established for this node's other metadata predicates.
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (1, 'alignmentPredicate', 1, FALSE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
