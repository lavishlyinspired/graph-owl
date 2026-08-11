-- Registered finding rules for the native reconcile engine — Epic 105 P5b
-- (`plans/105b-native-reconcile-engine.md`).
--
-- The third registry in this shape, after `predicates` (V3) and `namespaces`
-- (V14): a pack's `[[findings]]` rules are declared here at install time
-- rather than parsed from `pack.toml` by the engine itself, so the reconcile
-- endpoint never touches a manifest or the filesystem.
CREATE TABLE finding_rules (
    pack        TEXT NOT NULL,
    label       TEXT NOT NULL,

    summary     TEXT NOT NULL,
    governed_by TEXT NOT NULL,

    -- The SPARQL text, inlined at registration time. Not a path: this
    -- registry, like its two siblings, never reads a pack's files.
    query       TEXT NOT NULL,

    -- The variable in `query`'s result set naming the finding's subject.
    subject_var TEXT NOT NULL,

    -- [{predicate, var}, ...] — which evidence to attach, and which
    -- predicate each remaining binding is evidence *of*. A reviewer follows
    -- evidence back into the graph; evidence that cannot be followed is
    -- worse than none.
    evidence    JSONB NOT NULL,

    -- Opaque: `graph_owl_resolution::rule_match`'s `SimilarityBand`/
    -- `SpanCondition` shapes, kept untyped here so this table is not a
    -- second, driftable definition of what actually evaluates them.
    similarity  JSONB,
    span        JSONB,

    declared_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (pack, label)
);

-- **Upsert on `(pack, label)` is the intended access pattern, not a conflict
-- to refuse.** Unlike `namespaces` (a code is permanent once flakes are
-- written against it) or `predicates` (a value-type change would make
-- stored flakes unreadable), nothing else in the graph is keyed to a rule's
-- current query text — so reloading a pack whose author edited a rule is a
-- normal update, not a redefinition the registry must protect against.
COMMENT ON TABLE finding_rules IS
    'Registered finding rules for the native reconcile engine. Upsert on '
    '(pack, label): a rule carries no stored artifact a changed query would '
    'invalidate, unlike the namespace and predicate registries beside it.';
