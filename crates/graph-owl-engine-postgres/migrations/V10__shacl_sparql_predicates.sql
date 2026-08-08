-- `sh:sparql`/`sh:SPARQLConstraint` — Epic 96 Slice A, found missing while
-- wiring the bare constraint into `POST /validation/runs` (Phase 1.1 of
-- `plans/EPIC-COMPLETION-PLAN.md`). V5 seeded every other SHACL predicate
-- the shape reader understands; these two were never added, so a real
-- Postgres-backed store refuses to assert a `sh:sparql` shape at all
-- (`UnregisteredPredicate`) — the in-memory test fake used by
-- `graph_owl_api`'s own unit tests does not enforce registration, which is
-- why this went unnoticed until a real end-to-end HTTP test tried it.
--
-- namespace 260 = shacl: (`namespace::SHACL` in graph-owl-core).
--
-- `sparql` is `many` — `graph_owl_constraint::shapes`'s own reader comment:
-- "A shape may carry more than one." `select` is single-valued: one query
-- text per `sh:SPARQLConstraint` node.
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (260, 'sparql', 0, TRUE,  TRUE),
    (260, 'select', 1, FALSE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
