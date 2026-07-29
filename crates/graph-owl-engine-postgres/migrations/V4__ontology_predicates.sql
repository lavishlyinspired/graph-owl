-- The vocabulary an ontology is stated in — Epic 6.
--
-- Every write goes through the predicate registry, so without these rows there
-- is no way to assert `C1 rdfs:subClassOf C2` at all, and the reasoner has
-- nothing to reason over. `rdf:type` was already seeded in V3; these are the
-- six the eight built-in rules read.
--
-- All are `ref`-valued (value_type 0): each relates one identifier to another,
-- and a literal on either side would be a statement the rules cannot use.
--
-- All are `many` (TRUE): a class may specialise several superclasses, a
-- property may have several inverses stated over time, and an entity may be
-- declared the same as more than one other. Marking any of them single-valued
-- would make the *second* assertion silently supersede the first, which is
-- exactly the kind of quiet loss an ontology cannot survive.
--
-- `core` is TRUE: these are part of the shipped vocabulary, not something a
-- deployment registered, so nothing may retire them.
--
-- namespace 257 = rdfs:, 259 = owl: (see `namespace` in graph-owl-core).
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (257, 'subClassOf',    0, TRUE, TRUE),
    (257, 'subPropertyOf', 0, TRUE, TRUE),
    (257, 'domain',        0, TRUE, TRUE),
    (257, 'range',         0, TRUE, TRUE),
    (259, 'inverseOf',     0, TRUE, TRUE),
    (259, 'sameAs',        0, TRUE, TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
