-- A dismissal must survive the next scheduled run — Epic 105 P5.
--
-- V59 keyed the open-finding index on `(pack, label, subject)` and made it
-- partial (`WHERE status = 'pending'`), reasoning that a recurrence deserves
-- to be seen again. Running the real GST reconciliation twice around a
-- decision showed what that means in practice: a finding dismissed with a
-- reason came straight back on the next run over *identical* data. A reviewer
-- who dismisses something on Monday and sees it again unchanged on Tuesday
-- stops reading the queue, which costs far more than a duplicate would.
--
-- The evidence digest draws the line where it belongs. The same conclusion
-- from the same facts is the one already decided; the same conclusion from
-- changed facts is a new situation the reviewer must see. So the index covers
-- every status and gains the digest.
ALTER TABLE findings
    ADD COLUMN evidence_digest TEXT NOT NULL DEFAULT '';

DROP INDEX IF EXISTS findings_pending_unique;

-- Not partial: a decided finding must go on suppressing an identical
-- re-derivation, which is the whole correction.
CREATE UNIQUE INDEX findings_identity_unique
    ON findings (pack, label, subject, evidence_digest);
