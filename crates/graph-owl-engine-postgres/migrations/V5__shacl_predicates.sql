-- The vocabulary a shape is stated in — Epic 5 Slice B.
--
-- Every write goes through the predicate registry, so without these rows a
-- shape cannot be written into the graph at all and constraint validation has
-- nothing to run. Same reason V4 seeds the OWL/RDFS axiom predicates.
--
-- namespace 260 = shacl: (`namespace::SHACL` in graph-owl-core).
--
-- **`sh:in` is `many`, and that is a stated departure from the specification.**
-- SHACL encodes a value list as an `rdf:first`/`rdf:rest` chain; this repeats
-- the predicate instead, which says the same thing in the flake model's own
-- idiom and costs two fewer nodes per member. `crates/graph-owl-constraint/src/shapes.rs`
-- documents it, and `plans/00k-standards-conformance.md` records it as a
-- non-conformance rather than an omission.
--
-- `sh:property` is `many` because a node shape constrains several paths;
-- `sh:targetNode` because SHACL's own `sh:targetNode` is a list of nodes.
-- Everything else is single-valued: two `sh:minCount` values on one property
-- shape is a shape somebody edited badly, and the second silently winning is
-- how a constraint quietly loosens.
--
-- value_type: 0=ref 1=str 2=bool 3=int 4=float 5=instant 6=json
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    -- structure
    (260, 'NodeShape',        0, FALSE, TRUE),
    (260, 'property',         0, TRUE,  TRUE),
    (260, 'path',             0, FALSE, TRUE),
    -- targets
    (260, 'targetClass',      0, FALSE, TRUE),
    (260, 'targetNode',       0, TRUE,  TRUE),
    (260, 'targetSubjectsOf', 0, FALSE, TRUE),
    (260, 'targetObjectsOf',  0, FALSE, TRUE),
    -- annotation
    (260, 'severity',         1, FALSE, TRUE),
    (260, 'message',          1, FALSE, TRUE),
    -- constraints. `hasValue` and `in` are `ref`-typed in the registry but hold
    -- whatever the constrained predicate holds, so they are the two the
    -- registry cannot usefully type-check; the shape reader checks them instead.
    (260, 'minCount',         3, FALSE, TRUE),
    (260, 'maxCount',         3, FALSE, TRUE),
    (260, 'minLength',        3, FALSE, TRUE),
    (260, 'maxLength',        3, FALSE, TRUE),
    (260, 'minInclusive',     4, FALSE, TRUE),
    (260, 'maxInclusive',     4, FALSE, TRUE),
    (260, 'pattern',          1, FALSE, TRUE),
    (260, 'datatype',         1, FALSE, TRUE),
    (260, 'nodeKind',         1, FALSE, TRUE),
    (260, 'class',            0, FALSE, TRUE),
    (260, 'hasValue',         1, FALSE, TRUE),
    (260, 'in',               1, TRUE,  TRUE)
ON CONFLICT (namespace, name) DO NOTHING;
