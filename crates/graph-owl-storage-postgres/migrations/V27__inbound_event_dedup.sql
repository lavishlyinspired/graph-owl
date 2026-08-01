-- Epic 18 Slice B: dedup and last-writer-wins ordering.
--
-- `dedup_key` is what `graph_owl_core::webhook::dedup_key` computed for the
-- delivery — the sender's own event id when it provides one, else a content
-- hash of the raw bytes. Stored on every row (not just the first) so a
-- replay later compares against exactly the key a redelivery was judged
-- against.
ALTER TABLE inbound_events ADD COLUMN dedup_key TEXT NOT NULL;

-- The dedup marker itself. `PRIMARY KEY (endpoint_id, dedup_key)` is what
-- makes "concurrent duplicate deliveries produce one effect" true under
-- concurrency rather than merely likely: two simultaneous `INSERT`s racing
-- on the same key serialize at the database, and exactly one wins the row.
-- `first_event_id` is not itself the source of truth for anything yet — it
-- is kept because a duplicate's own row does not otherwise say which
-- delivery it was a duplicate *of*, which the dead-letter/replay work
-- (Slice D) will want.
CREATE TABLE inbound_event_dedup (
    endpoint_id    UUID NOT NULL REFERENCES webhook_endpoints (id) ON DELETE CASCADE,
    dedup_key      TEXT NOT NULL,
    first_event_id UUID NOT NULL REFERENCES inbound_events (id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (endpoint_id, dedup_key)
);
