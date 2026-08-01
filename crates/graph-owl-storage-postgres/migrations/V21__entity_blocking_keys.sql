-- Epic 17 Slice B: blocking keys for entity-resolution candidate generation.
--
-- One row per (asset, key_type). Four key types per asset, computed and
-- upserted whenever `upsert_asset` writes: `normalized_fqn`, `name_parent`,
-- `soundex_name`, `column_hash`. Candidate generation is then a lookup
-- against `entity_blocking_keys_lookup` rather than a comparison over every
-- asset — the same asset sharing a key with itself is excluded by the
-- caller, not by this schema.
CREATE TABLE entity_blocking_keys (
    asset_id  UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    key_type  TEXT NOT NULL CHECK (
        key_type IN ('normalized_fqn', 'name_parent', 'soundex_name', 'column_hash')
    ),
    key_value TEXT NOT NULL,
    PRIMARY KEY (asset_id, key_type)
);

-- The candidate-generation lookup: given a (key_type, key_value) pair, find
-- every other asset sharing it. This is the index Slice B's acceptance
-- criteria require a query plan to name.
CREATE INDEX entity_blocking_keys_lookup ON entity_blocking_keys (key_type, key_value);
