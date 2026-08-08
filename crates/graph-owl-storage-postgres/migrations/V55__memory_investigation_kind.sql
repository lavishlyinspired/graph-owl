-- Epic 32 (Phase 3 completion): `record_investigation` was always meant to
-- assert `MemoryKind::Investigation` (named in plans/32-agent-capabilities.md
-- from the start), but the variant — and this constraint — were never added.
-- Widening rather than replacing the check: every existing row's kind stays
-- valid, this only admits one more.
ALTER TABLE memories DROP CONSTRAINT memories_kind_check;
ALTER TABLE memories ADD CONSTRAINT memories_kind_check
    CHECK (kind IN ('rationale', 'incident', 'decision', 'caveat', 'investigation'));
