-- Epic 17 Slice F: the review queue. `UNIQUE (target_id, candidate_id)` is
-- what makes `queue_for_review` idempotent — an `ON CONFLICT DO NOTHING`
-- write, so an existing entry (pending, confirmed, or rejected) is never
-- overwritten by a later re-resolution of the same draft.
CREATE TABLE resolution_queue (
    id           UUID PRIMARY KEY,
    target_id    UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    candidate_id UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    score        DOUBLE PRECISION NOT NULL,
    evidence     JSONB NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('pending', 'confirmed', 'rejected')),
    decided_by   JSONB,
    decided_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (target_id, candidate_id)
);

-- The working queue's own query: pending first, worst offenders (highest
-- score) first within that.
CREATE INDEX resolution_queue_status_score ON resolution_queue (status, score DESC);
