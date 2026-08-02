-- Epic 22: organization-defined fields on entity types.

CREATE TABLE custom_properties (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    -- **Per entity type** (decision 2). `costCenter` on a table need not exist
    -- on a user, and a globally-scoped vocabulary would force every
    -- organization's fields onto every entity.
    entity_type   TEXT NOT NULL,
    property_type TEXT NOT NULL,
    description   TEXT,
    -- Bounds as JSONB rather than columns: the set of constraints differs per
    -- type (an enum has values, a number has a range), so columns would be
    -- mostly-null and each new constraint would be a migration. Nothing filters
    -- on them, so there is no index to lose.
    constraints   JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Uniqueness is **scoped to the entity type**, which is the whole of decision 2
-- expressed as a constraint: the same name on two types is two different
-- properties, and a global unique index would silently forbid that.
CREATE UNIQUE INDEX custom_properties_name_per_type
    ON custom_properties (entity_type, name);

CREATE INDEX custom_properties_by_type ON custom_properties (entity_type);

-- **A separate column from `properties`, and the separation is load-bearing.**
--
-- `properties` is what the *source system* reported — a column's data type, a
-- service's engine — and the connector upsert replaces it wholesale
-- (`properties = COALESCE(EXCLUDED.properties, assets.properties)`). `extension`
-- is what the *organization* added. Had custom properties gone into
-- `properties`, the next connector run would have silently wiped every
-- hand-curated `costCenter`, which is exactly the class of silent data loss
-- this schema refuses everywhere else.
ALTER TABLE assets ADD COLUMN extension JSONB NOT NULL DEFAULT '{}';

-- GIN, because filtering is `?extension.costCenter=CC-1234` over an open set of
-- keys. Per-definition columns would mean a migration per property, which
-- defeats the entire purpose of the feature; a GIN index over the whole
-- document supports containment queries on any key without one.
CREATE INDEX assets_extension ON assets USING GIN (extension jsonb_path_ops);
