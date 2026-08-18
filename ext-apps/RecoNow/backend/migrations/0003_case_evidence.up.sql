-- Plan 122b B4: "why this case exists" needs the real fields GraphOWL's
-- own Finding struct carries (crates/graph-owl-core/src/finding.rs) —
-- label (the rule kind), governed_by (statute/policy, never absent),
-- summary (one line), subject (the graph subject, for the "Open in
-- GraphOWL" deep link), and evidence_count (len(finding.evidence), never
-- empty per that struct's own doc comment). No confidence score: the
-- Finding struct does not track one, so this schema does not invent one
-- either.
ALTER TABLE case_record ADD COLUMN subject TEXT;
ALTER TABLE case_record ADD COLUMN summary TEXT;
ALTER TABLE case_record ADD COLUMN governed_by TEXT;
ALTER TABLE case_record ADD COLUMN evidence_count INTEGER;
