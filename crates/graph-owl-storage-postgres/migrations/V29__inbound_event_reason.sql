-- Epic 18 Slice D: dead-letter reasons.
--
-- Nullable, and always written alongside a state transition — `Some(...)`
-- moving to `Failed`, `None` for every other state. A successful replay
-- clears it: a stale reason left on a now-`Applied` row would read as still
-- failing.
ALTER TABLE inbound_events ADD COLUMN reason TEXT;
