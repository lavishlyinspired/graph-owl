-- Epic 17 Slice D/E: reversible merges. A merge is a record with a
-- `split_at`, not a destructive rewrite (`17-entity-resolution.md` decision
-- 1) — splitting sets a column, it never deletes the row.
CREATE TABLE merge_records (
    id           UUID PRIMARY KEY,
    canonical_id UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    merged_id    UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    evidence     JSONB NOT NULL,
    confidence   DOUBLE PRECISION NOT NULL,
    decided_by   JSONB NOT NULL,
    decided_at   TIMESTAMPTZ NOT NULL,
    -- The engine transaction time the merge wrote at, so a split can restore
    -- exactly the pre-merge state via `as_of: merged_at_t - 1` rather than
    -- reconstructing it from wall-clock time.
    merged_at_t  BIGINT NOT NULL,
    split_at     TIMESTAMPTZ
);

-- The cooldown check (Slice E) looks up the most recent split for a pair in
-- either role, so both sides of the relationship need an index.
CREATE INDEX merge_records_canonical ON merge_records (canonical_id);
CREATE INDEX merge_records_merged ON merge_records (merged_id);
