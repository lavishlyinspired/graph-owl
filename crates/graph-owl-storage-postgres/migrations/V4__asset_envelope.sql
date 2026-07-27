-- The entity envelope. Applied to five asset kinds now; twenty-five later,
-- which is the whole reason it lands before more entity types (ROADMAP.md).
ALTER TABLE assets
    ADD COLUMN version_major INT NOT NULL DEFAULT 0,
    ADD COLUMN version_minor INT NOT NULL DEFAULT 1,
    ADD COLUMN updated_by TEXT NOT NULL DEFAULT 'system',
    ADD COLUMN change_description JSONB,
    -- Soft delete. A tombstone is the truth about a table that used to exist;
    -- hard-deleting it makes the fact of its removal unrecoverable.
    ADD COLUMN deleted BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN deleted_at TIMESTAMPTZ;

-- Live assets only, which is every list and search query's default.
CREATE INDEX assets_live_fqn ON assets (fully_qualified_name, id) WHERE NOT deleted;

-- One row per version. Unbounded by decision: pruning an audit trail needs
-- evidence, and there is none yet (03-versioning.md decision 6).
CREATE TABLE asset_versions (
    asset_id           UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    version_major      INT NOT NULL,
    version_minor      INT NOT NULL,
    -- The whole entity as it stood, not a patch. A snapshot answers "what did
    -- this look like" without replaying every diff from the beginning.
    snapshot           JSONB NOT NULL,
    change_description JSONB,
    updated_by         TEXT NOT NULL,
    updated_at         TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (asset_id, version_major, version_minor)
);

CREATE INDEX asset_versions_recent ON asset_versions (asset_id, updated_at DESC);
