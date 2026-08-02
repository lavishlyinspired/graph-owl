-- Epic 19 Slice D: streamed messages that failed `poison_threshold` apply
-- attempts. A separate table from Epic 18's `inbound_events`, not a reuse:
-- that table's rows are webhook deliveries with a hard FK to
-- `webhook_endpoints` and a signature-verification lifecycle
-- (Received/Mapped/...), none of which a broker message has — forcing one
-- into the other's shape would leave half the columns lying about what
-- happened.
CREATE TABLE stream_dead_letters (
    id              UUID PRIMARY KEY,
    -- CASCADE: a dead letter is meaningless once its subscription is gone —
    -- there is no mapping left to fix and replay it against.
    subscription_id UUID NOT NULL REFERENCES stream_subscriptions (id) ON DELETE CASCADE,
    topic           TEXT NOT NULL,
    partition       INTEGER NOT NULL,
    -- `kafka_offset`, not `offset`: OFFSET is a reserved word.
    kafka_offset    BIGINT NOT NULL,
    payload         BYTEA NOT NULL,
    reason          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX stream_dead_letters_subscription ON stream_dead_letters (subscription_id);
