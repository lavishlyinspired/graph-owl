-- Epic 16 Slice B: a retried push converges.
--
-- Decision 4 calls this **mandatory for push, not optional**: at-least-once
-- transport (Epic 18) duplicates without it, and a pusher that times out and
-- retries has no way to know whether the first attempt landed.
CREATE TABLE idempotency_keys (
    key TEXT PRIMARY KEY,

    -- **A key identifies a request, not a slot.** Reusing a key for different
    -- content is a client bug — usually a key generated once and reused across a
    -- loop — and serving the first response for the second body would silently
    -- drop a push the client believes succeeded. The hash is what lets that be
    -- reported rather than hidden.
    request_hash TEXT NOT NULL,

    -- The original answer, replayed verbatim. Storing the rendered body rather
    -- than re-deriving it means a replay cannot disagree with the first response
    -- because something in the estate changed in between.
    status  SMALLINT NOT NULL,
    body    JSONB NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Keys expire after 24h. Swept on write rather than by a background job: there is
-- no scheduler (Epic 15 decision 5 refuses one), and a table that only grows is a
-- slow leak nobody notices until it is large.
CREATE INDEX idempotency_keys_by_age ON idempotency_keys (created_at);
