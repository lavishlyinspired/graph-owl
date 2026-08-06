-- Epic 42 decision 3: a rejection is auditable only if the reason survives
-- past the request that made it. Nullable — a confirmed or still-pending
-- entry has none, and forcing one would misdescribe those states.
ALTER TABLE resolution_queue ADD COLUMN reason TEXT;
