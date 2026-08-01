-- Epic 18 Slice C: versioned webhook payload-to-draft mappings.
--
-- Every update is a new row, never an `UPDATE` in place — `UNIQUE (name,
-- version)` is what makes "mappings are versioned so a fix is auditable"
-- true structurally: the old rule is still a row, not overwritten data a
-- reviewer would need a WAL to recover.
CREATE TABLE mapping_versions (
    id                 UUID PRIMARY KEY,
    name               TEXT NOT NULL CHECK (name <> ''),
    version            INTEGER NOT NULL CHECK (version > 0),
    kind_expr          JSONB NOT NULL,
    name_expr          JSONB NOT NULL,
    parent_fqn_expr    JSONB,
    description_expr   JSONB,
    properties_exprs   JSONB NOT NULL DEFAULT '{}',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (name, version)
);

CREATE INDEX mapping_versions_by_name ON mapping_versions (name, version DESC);
