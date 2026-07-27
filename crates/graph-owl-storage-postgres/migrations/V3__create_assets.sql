-- The asset hierarchy: service -> database -> schema -> table -> column.
-- One table for all five kinds. Five tables would mean five repositories and
-- five sets of near-identical SQL for one concept, and adding a sixth kind
-- (Epic 34) would be a migration rather than an enum value.
CREATE TABLE assets (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind <> ''),
    name TEXT NOT NULL CHECK (name <> ''),
    fully_qualified_name TEXT NOT NULL UNIQUE CHECK (fully_qualified_name <> ''),
    -- Self-referential. ON DELETE CASCADE so removing a schema removes its
    -- tables and their columns: an orphaned column addresses nothing and would
    -- be invisible to every hierarchy query while still occupying an FQN.
    parent_id UUID REFERENCES assets (id) ON DELETE CASCADE,
    description TEXT,
    properties JSONB,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

-- Children of a parent, in name order. The hierarchy walk the UI and the
-- connector both perform.
CREATE INDEX assets_parent_name ON assets (parent_id, name);

-- Keyset pagination and search both order by FQN.
CREATE INDEX assets_kind_fqn ON assets (kind, fully_qualified_name);

-- Substring search over names, for Epic 8's lexical path and the console's
-- search box until then.
CREATE INDEX assets_name_trgm ON assets (lower(name) text_pattern_ops);
