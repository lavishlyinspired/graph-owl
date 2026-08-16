-- The rule's own priority, copied onto every finding it produces at the
-- moment it was filed — Epic 105 P10 (`plans/119-architecture-audit.md`
-- §10), the sibling of `finding_rules.priority`
-- (`graph-owl-engine-postgres` V17).
--
-- **Copied once, not re-read from the rule.** `findings` upserts with
-- `ON CONFLICT DO NOTHING` (V59's identity index), the same as
-- `summary`/`governed_by` already do — a later edit to the rule's declared
-- priority must not silently reorder a finding a reviewer is already
-- looking at.
ALTER TABLE findings
    ADD COLUMN priority SMALLINT;
