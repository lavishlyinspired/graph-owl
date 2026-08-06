-- Epic 42 decision 2/3: a rejected extraction claim carries a reason, the
-- same as a rejected review-queue entry (V49). Nullable — accepted, edited,
-- and pending claims have none.
ALTER TABLE extraction_claims ADD COLUMN reason TEXT;
