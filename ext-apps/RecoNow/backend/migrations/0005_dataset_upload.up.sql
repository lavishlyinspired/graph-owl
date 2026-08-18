-- Uploaded datasets lived only in a module-level `WORKSPACES` dict, so a
-- refresh of the browser was fine but a backend restart silently dropped
-- every uploaded file, and there was no way to reopen a file's mapping after
-- navigating away — the mapping screen only ever showed the file just picked.
--
-- Storing the parsed rows (rather than the original bytes) keeps this table
-- the same shape the reconcile path already reads, so nothing else changes.
-- Rows are bounded by what a period's returns contain; if that ever stops
-- being true the fix is a row table, not a blob, and this comment is the
-- marker for it.
CREATE TABLE dataset_upload (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id    UUID NOT NULL REFERENCES client(id) ON DELETE CASCADE,
    period_id    UUID NOT NULL REFERENCES period(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    headers      JSONB NOT NULL,
    rows         JSONB NOT NULL,
    total_rows   INTEGER NOT NULL,
    mapping      JSONB NOT NULL,
    confirmed    BOOLEAN NOT NULL DEFAULT FALSE,
    uploaded_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One current file per kind per period: re-uploading replaces.
    UNIQUE (client_id, period_id, kind)
);
