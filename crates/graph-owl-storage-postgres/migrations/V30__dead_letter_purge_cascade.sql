-- Epic 18 Slice D: purging a dead-lettered event must not leave a dangling
-- dedup marker behind.
--
-- A `Failed` event can be the `first_event_id` its own `inbound_event_dedup`
-- row points to (it was the first, and only, delivery for that key, and
-- processing it failed). Deleting it without `ON DELETE CASCADE` violates
-- the foreign key — found by `purging_removes_only_old_failed_events`, not
-- by inspection. Once the event is gone, the marker is moot anyway: nothing
-- remembers this delivery either way, so a genuine redelivery after a purge
-- is correctly treated as new, not incorrectly blocked as a duplicate of a
-- row nobody can look up any more.
ALTER TABLE inbound_event_dedup
    DROP CONSTRAINT inbound_event_dedup_first_event_id_fkey,
    ADD CONSTRAINT inbound_event_dedup_first_event_id_fkey
        FOREIGN KEY (first_event_id) REFERENCES inbound_events (id) ON DELETE CASCADE;
