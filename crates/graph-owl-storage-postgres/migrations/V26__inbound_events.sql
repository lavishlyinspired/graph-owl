-- Epic 18: inbound webhook events, from signature verification onward.
-- `sender_event_id`/`sender_timestamp` are nullable — not every sender
-- provides either, and Slice B's dedup/ordering falls back accordingly.
CREATE TABLE inbound_events (
    id                UUID PRIMARY KEY,
    endpoint_id       UUID NOT NULL REFERENCES webhook_endpoints (id) ON DELETE CASCADE,
    sender_event_id   TEXT,
    sender_timestamp  TIMESTAMPTZ,
    received_at       TIMESTAMPTZ NOT NULL,
    raw               BYTEA NOT NULL,
    state             TEXT NOT NULL CHECK (
        state IN ('received', 'mapped', 'applied', 'failed', 'duplicate')
    )
);

CREATE INDEX inbound_events_endpoint ON inbound_events (endpoint_id, received_at DESC);
