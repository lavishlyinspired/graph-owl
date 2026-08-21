-- Epic 31 follow-on: a memory link can now point at a graph-native subject
-- (a domain pack's own IRI — an invoice, a filing period), not only a
-- catalog asset or another memory.
--
-- Verified against the running deployment: a real GST finding's subject
-- (`https://graph-owl.dev/packs/gst#books-27AABCS1429B1Z8-INV-MAR-011`) is
-- exactly this shape, and `/assets/{id}/contradictions` and
-- `/assets/{id}/memories` returned `400 UUID parsing failed` against it
-- before this existed — there was no `Uuid` column that string could ever
-- go in.
--
-- **No foreign key**, matching `findings.subject` (V59) exactly: a graph
-- subject lives as triples, not as a row in a relational table with a
-- stable primary key this column could reference. `asset_target` and
-- `memory_target` keep their real FKs — this is a third, deliberately
-- weaker column for a third, deliberately different kind of target.
ALTER TABLE memory_links ADD COLUMN subject_target TEXT;

ALTER TABLE memory_links DROP CONSTRAINT memory_links_one_target;
ALTER TABLE memory_links ADD CONSTRAINT memory_links_one_target CHECK (
    (asset_target IS NOT NULL AND memory_target IS NULL AND subject_target IS NULL)
    OR
    (asset_target IS NULL AND memory_target IS NOT NULL AND subject_target IS NULL)
    OR
    (asset_target IS NULL AND memory_target IS NULL AND subject_target IS NOT NULL AND subject_target <> '')
);

CREATE UNIQUE INDEX memory_links_subject_identity
    ON memory_links (memory_id, relation, subject_target) WHERE subject_target IS NOT NULL;

-- Mirrors `memory_links_by_asset`/`memory_links_by_memory` (V15) — "what do
-- we know about this subject" is the same retrieval question, for the third
-- kind of target.
CREATE INDEX memory_links_by_subject ON memory_links (subject_target) WHERE subject_target IS NOT NULL;
