-- Column-level lineage — Epic 29 Slice D.
--
-- "Which source column produced this number" is the question that ends a
-- data-quality argument, and table-level lineage cannot answer it.

CREATE TABLE lineage_column_mappings (
    edge_id         UUID NOT NULL REFERENCES lineage_edges (id) ON DELETE CASCADE,

    -- **Column FQNs, and the mapping is many-to-one.** `first_name` and
    -- `last_name` → `full_name` is the ordinary case, not an edge case, and a
    -- one-to-one model breaks on the first concatenation anybody catalogues.
    -- One row per source column, so the many side needs no array and no
    -- ordering nobody agreed on.
    from_column_fqn TEXT NOT NULL CHECK (from_column_fqn <> ''),
    to_column_fqn   TEXT NOT NULL CHECK (to_column_fqn <> ''),

    -- How, when the producer knows: `concat(first_name, ' ', last_name)`. The
    -- same reason `lineage_edges.query` exists — a mapping carrying its
    -- expression can be checked, one without is an assertion a reader must take
    -- on trust.
    expression      TEXT,

    PRIMARY KEY (edge_id, from_column_fqn, to_column_fqn)
);

-- "What feeds this column" reads this way, which is the direction a
-- data-quality argument runs.
CREATE INDEX lineage_column_mappings_by_target ON lineage_column_mappings (to_column_fqn);
CREATE INDEX lineage_column_mappings_by_source ON lineage_column_mappings (from_column_fqn);
