-- Plan 109 Slice 1: reversible attachments of a domain-pack source record to
-- a canonical domain-pack subject. `MergeRecord`'s (V22) counterpart for
-- subjects rather than catalog assets.
--
-- **`canonical`/`attached` are TEXT, not foreign keys to `assets` —
-- mirrors `findings.subject` (V59) for the same reason.** A domain-pack
-- subject (a `gst:Invoice`, say) is a graph subject with no asset row by
-- design (`plans/105-domain-neutrality.md` DN-3); a foreign key here would
-- force every domain entity to become a catalog asset.
--
-- **Reversible, not a destructive rewrite** — the same contract
-- `merge_records.split_at` already carries: splitting sets a column, it
-- never deletes the row.
CREATE TABLE subject_attachments (
    id            UUID PRIMARY KEY,
    canonical     TEXT NOT NULL,
    attached      TEXT NOT NULL,
    evidence      JSONB NOT NULL,
    confidence    DOUBLE PRECISION NOT NULL,
    decided_by    JSONB NOT NULL,
    decided_at    TIMESTAMPTZ NOT NULL,
    -- The engine transaction time the attachment wrote at, so a split can
    -- restore exactly the pre-attachment state via
    -- `as_of: attached_at_t - 1` rather than reconstructing it from
    -- wall-clock time — the same reason `merge_records.merged_at_t` exists.
    attached_at_t BIGINT NOT NULL,
    split_at      TIMESTAMPTZ
);

CREATE INDEX subject_attachments_canonical ON subject_attachments (canonical);
CREATE INDEX subject_attachments_attached ON subject_attachments (attached);
