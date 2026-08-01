-- Epic 17 Slice G: mention resolution. `source_id` has no foreign key — a
-- mention's source is whatever kind of document it was extracted from (most
-- commonly a memory), and constraining it to one entity type would make
-- this reusable only for that type.
CREATE TABLE mention_resolutions (
    id          UUID PRIMARY KEY,
    source_id   UUID NOT NULL,
    text        TEXT NOT NULL,
    entity_id   UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    confidence  DOUBLE PRECISION NOT NULL,
    resolved_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX mention_resolutions_source ON mention_resolutions (source_id, resolved_at DESC);
