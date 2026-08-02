-- Epic 18 Slice D (extended): out-of-order protection.
--
-- `inbound_events.state` gains `superseded` — an older `sender_timestamp`
-- than what is already applied, recognized and deliberately not applied.
ALTER TABLE inbound_events DROP CONSTRAINT inbound_events_state_check;
ALTER TABLE inbound_events ADD CONSTRAINT inbound_events_state_check CHECK (
    state IN ('received', 'mapped', 'applied', 'failed', 'duplicate', 'superseded')
);

-- The high-water mark a candidate's `sender_timestamp` is compared against.
-- Keyed by the entity's `fully_qualified_name` rather than its id: the
-- comparison has to happen *before* the entity necessarily exists (the
-- first-ever delivery for a not-yet-created entity has nothing to compare
-- against), and the FQN is what a mapping resolves to, not an id.
CREATE TABLE entity_last_applied (
    fully_qualified_name TEXT PRIMARY KEY,
    sender_timestamp      TIMESTAMPTZ NOT NULL,
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
