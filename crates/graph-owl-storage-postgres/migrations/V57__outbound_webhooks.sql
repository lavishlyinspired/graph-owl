-- Epic 14 Slice F / decision 4.2 (EPIC-COMPLETION-PLAN.md, 8 Aug 2026):
-- outbound webhook subscriptions and their delivery queue. The pure
-- signing/backoff/target-admission logic already lives in
-- graph_owl_events::webhook; this is the persistence half a real EventSink
-- needs to enqueue against.
CREATE TABLE outbound_webhooks (
    id           UUID PRIMARY KEY,
    url          TEXT NOT NULL CHECK (url <> ''),
    -- Raw HMAC key material, never a hash — signing needs to recompute the
    -- same MAC a receiver will check. Selected only by the one read path
    -- that signs a delivery; list/get routes never return it.
    --
    -- Nullable at the schema level even though every row this project ever
    -- writes ends up with one: `upsert_outbound_webhook` uses the same
    -- `secret = COALESCE($n, table.secret)` upsert idiom
    -- `upsert_webhook_endpoint`/`upsert_stream_subscription` already use, and
    -- Postgres validates a `NOT NULL` constraint against the *proposed*
    -- insert row of `INSERT ... ON CONFLICT DO UPDATE` even when the
    -- conflict resolves to the UPDATE branch — so a `NOT NULL` column here
    -- broke every update-without-a-new-secret, the exact case this idiom
    -- exists for. "A secret is required on first registration" is instead
    -- an application-level check (the Postgres and in-memory adapters both
    -- confirm a row already exists before accepting `secret: None`).
    secret       BYTEA,
    -- Empty means every kind — graph_owl_events::webhook::WebhookRegistration
    -- ::wants's own convention, reused rather than re-decided here.
    event_types  TEXT[] NOT NULL DEFAULT '{}',
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per (subscription, event) pair a matching change enqueued.
-- `attempt`/`next_attempt_at`/`dead_lettered` are the sender's own state
-- machine (Slice B, not yet built) — this migration lands the queue a
-- sender will later drain, proven correct by what enqueues into it now.
CREATE TABLE outbound_webhook_deliveries (
    id              UUID PRIMARY KEY,
    webhook_id      UUID NOT NULL REFERENCES outbound_webhooks(id) ON DELETE CASCADE,
    payload         JSONB NOT NULL,
    attempt         INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error      TEXT,
    dead_lettered   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The sender's own future query shape: pending work, oldest first. Partial
-- on `NOT dead_lettered` so a growing dead-letter tail never bloats the
-- index the hot path actually reads.
CREATE INDEX outbound_webhook_deliveries_pending
    ON outbound_webhook_deliveries (next_attempt_at)
    WHERE NOT dead_lettered;
