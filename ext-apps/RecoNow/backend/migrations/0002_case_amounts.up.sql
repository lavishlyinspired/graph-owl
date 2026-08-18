-- Plan 122b B2: the dashboard's ITC cards and B4's register both need a
-- case to carry money, not just a status — added here rather than in
-- 0001 because the need only became concrete once a screen had to render
-- real totals. All nullable: a case created before this migration, or one
-- the reconcile bridge could not extract an amount for, is not an error.
ALTER TABLE case_record ADD COLUMN supplier_name TEXT;
ALTER TABLE case_record ADD COLUMN supplier_gstin TEXT;
ALTER TABLE case_record ADD COLUMN books_amount NUMERIC;
ALTER TABLE case_record ADD COLUMN portal_amount NUMERIC;
