-- A rule's own rank against a pack's other rules — Epic 105 P10
-- (`plans/119-architecture-audit.md` §10).
--
-- Exists for a consumer that must collapse several findings on one subject
-- into a single decision (reco-now's one-row-per-invoice table is the
-- motivating case: an invoice can be both filed-but-absent-from-2B and
-- genuinely mismatched against what was filed, and one of the two has to
-- win). That ranking is a property of the finding *kind*, declared once
-- here by the pack, rather than a table of finding labels every consumer
-- re-derives for itself. graph-owl's own console has no such constraint —
-- every finding is its own row there — so this is read, never required.
--
-- Nullable: a rule that declares none is not an error, and is treated as
-- least urgent by any consumer that ranks by it.
ALTER TABLE finding_rules
    ADD COLUMN priority SMALLINT;
