-- Epic 15 decision 7: a re-run reads, compares and skips.
--
-- Nullable, and it stays nullable permanently. An asset catalogued before this
-- column existed has no fingerprint, and an asset created through the API has
-- none either — neither is a defect, and a NOT NULL default would invent a
-- fingerprint that matches nothing the source ever said. `Existing` in
-- graph-owl-connectors models the absence explicitly for the same reason: "no
-- such asset" and "an asset with no fingerprint" have different correct
-- answers.
--
-- BYTEA rather than TEXT: SHA-256 is 32 bytes, and hex would store 64 and
-- compare 64. The comparison runs once per source record on every run, which is
-- the hot path this whole column exists to shorten.
ALTER TABLE assets ADD COLUMN source_hash BYTEA;

-- No index. The lookup is always by `fully_qualified_name`, which is already
-- unique — the hash is read from the row that lookup returns, never searched
-- for. An index here would cost writes and serve nothing.
