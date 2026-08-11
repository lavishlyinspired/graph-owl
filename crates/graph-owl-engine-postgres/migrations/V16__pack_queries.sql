-- Registered named, parameterized pack queries — Epic 105 P106 Slice 4a
-- (`plans/106-agent-trace-hygiene.md`).
--
-- The fourth registry in this shape, after `predicates` (V3), `namespaces`
-- (V14) and `finding_rules` (V15): a pack's `[[queries]]` entries are
-- declared here at install time, the same way `finding_rules` already are,
-- so a caller can invoke one by name with runtime bindings without the
-- server ever reading a pack's files.
--
-- **Deliberately smaller than `finding_rules`.** A named query is a neutral
-- lookup, not a detector: it carries no `summary`/`governed_by` (nothing was
-- found, so there is nothing to explain to a reviewer) and no
-- `subject_var`/`evidence` (a lookup's shape is whatever the query itself
-- projects, not a finding's fixed subject-plus-evidence shape). Reusing
-- `finding_rules` for this would force those columns to mean nothing for
-- every row of this kind.
CREATE TABLE pack_queries (
    pack        TEXT NOT NULL,
    name        TEXT NOT NULL,

    -- The SPARQL text, inlined at registration time — never a path, matching
    -- every registry beside it.
    query       TEXT NOT NULL,

    declared_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (pack, name)
);

-- Upsert on `(pack, name)`, matching `finding_rules`' own comment: nothing
-- else in the graph is keyed to a query's current text, so reloading a pack
-- whose author edited a query is a normal update.
COMMENT ON TABLE pack_queries IS
    'Registered named, parameterized pack queries, invoked by name with '
    'runtime bindings. Upsert on (pack, name): a query carries no stored '
    'artifact a changed text would invalidate.';
