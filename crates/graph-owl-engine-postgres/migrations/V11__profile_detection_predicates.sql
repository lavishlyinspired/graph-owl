-- The OWL vocabulary Epics 98/99/100's profile-membership checks read —
-- found missing while wiring Epic 100's routing into `POST /reasoning/runs`
-- (`plans/EPIC-COMPLETION-PLAN.md` Phase 1.4). V4 seeded only the six
-- predicates the original eight RL rules read; every construct the RL/EL/QL
-- *detectors* check for — because they exist to find what is NOT
-- reasoner-safe — was never added. A real Postgres-backed store refuses to
-- assert any of them at all (`UnregisteredPredicate`), invisible until now
-- because `graph_owl_api`'s own unit tests seed fixtures against an
-- in-memory fake that does not enforce predicate registration — the
-- identical blind spot Phase 1.1 already found for `sh:sparql`/`sh:select`.
--
-- namespace 259 = owl: (see `namespace` in graph-owl-core).
--
-- `ref`-valued (value_type 0), `many` TRUE: each names another resource
-- (a restriction node, a property, a list), and — V4's own stated reason —
-- marking any of them single-valued would let a second assertion silently
-- supersede the first.
--
-- Cardinality predicates are the exception: `int`-valued (value_type 3),
-- `many` FALSE — a restriction states at most one value per cardinality
-- kind, the same single-valued shape `minCount`/`maxCount` already have in
-- V5's SHACL predicates.
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (259, 'disjointUnionOf',       0, TRUE,  TRUE),
    (259, 'hasKey',                0, TRUE,  TRUE),
    (259, 'propertyChainAxiom',    0, TRUE,  TRUE),
    (259, 'allValuesFrom',         0, TRUE,  TRUE),
    (259, 'unionOf',               0, TRUE,  TRUE),
    (259, 'complementOf',          0, TRUE,  TRUE),
    (259, 'cardinality',           3, FALSE, TRUE),
    (259, 'minCardinality',        3, FALSE, TRUE),
    (259, 'maxCardinality',        3, FALSE, TRUE),
    (259, 'qualifiedCardinality',  3, FALSE, TRUE),
    (259, 'minQualifiedCardinality', 3, FALSE, TRUE),
    (259, 'maxQualifiedCardinality', 3, FALSE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
