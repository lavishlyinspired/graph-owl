-- Epic 104 Slice D's cross-vocabulary alignment vocabulary — found missing
-- while wiring `POST /alignments` to HTTP
-- (`plans/EPIC-COMPLETION-PLAN.md` Phase 1.7). `graph_owl_ontology::alignment`
-- shipped, was correctly unit-tested, and never registered a single one of
-- its own predicates — invisible until now because `graph_owl_api`'s own
-- unit tests seed fixtures against an in-memory fake that does not enforce
-- predicate registration, the identical blind spot Phase 1.1 (`sh:sparql`)
-- and Phase 1.4 (twelve OWL profile-detection predicates) already found.
--
-- namespace 1 = dsc: (the reified alignment node's own metadata),
-- 259 = owl: (`owl:equivalentClass`, the direct triple `EquivalentClass`
-- writes), 267 = skos: (`exactMatch`/`closeMatch`/`broadMatch`/`narrowMatch`,
-- the direct triple `Match` writes).
--
-- All `many` FALSE: an alignment's reified node has exactly one of each
-- (one left, one right, one source, one confidence, one directionality) —
-- `Alignment::subject()` is deterministic per `(left, predicate, right)`
-- precisely so a later `upsert_alignment` call updates this same node
-- rather than accumulating a second value alongside the first.
--
-- value_type: 0=ref 1=str 2=bool 3=int 4=float 5=instant 6=json
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (1,   'alignmentLeft',        0, FALSE, TRUE),
    (1,   'alignmentRight',       0, FALSE, TRUE),
    (1,   'alignmentSourceKind',  1, FALSE, TRUE),
    (1,   'alignmentSourceDetail', 1, FALSE, TRUE),
    (1,   'lossyReverse',         2, FALSE, TRUE),
    (259, 'equivalentClass',      0, FALSE, TRUE),
    (267, 'exactMatch',           0, FALSE, TRUE),
    (267, 'closeMatch',           0, FALSE, TRUE),
    (267, 'broadMatch',           0, FALSE, TRUE),
    (267, 'narrowMatch',          0, FALSE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
