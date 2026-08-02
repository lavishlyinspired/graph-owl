-- The catalog vocabulary extraction is allowed to use — Epic 21 Slice G.
--
-- `graph_owl_api::extraction::CATALOG_PREDICATES` is what a worker's claims are
-- checked against, and an asserted claim is now projected into
-- `graph:extraction` as a flake. Every predicate in that vocabulary therefore
-- has to exist in this registry, or the claim passes review and then fails at
-- the write with `UnregisteredPredicate` — accepted by the policy and refused
-- by the engine, which is the worst of both. `term` and `dependsOn` were the
-- two the registry had never heard of.
--
-- Both are **references**, matching V6's `feeds` and `derivedFrom`: they name
-- another entity rather than carrying a value, so the object goes in OPST and
-- "what depends on this" is answerable by reverse traversal. A string here
-- would store fine and make the edge unreachable from the end that needed it.
--
-- Both are `many` for the same reason V6's are: a table depends on several
-- things and carries several glossary terms, and single-valued would make the
-- second assertion silently supersede the first.
--
-- value_type: 0=ref 1=str
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (1, 'term',      0, TRUE, TRUE),
    (1, 'dependsOn', 0, TRUE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
