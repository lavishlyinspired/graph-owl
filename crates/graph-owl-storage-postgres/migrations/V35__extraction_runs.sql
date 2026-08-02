-- Epic 21: extraction runs, the confirmation queue, and the record of what
-- was thrown away.
--
-- **A run is the unit of undo** (decision 0: "a bad run is deletable
-- wholesale"). That is only possible if every claim knows which run produced
-- it, hence a run identity rather than a loose pile of claims.

CREATE TABLE extraction_runs (
    id                  UUID PRIMARY KEY,
    source_id           TEXT        NOT NULL,
    -- Content hash of the source at extraction time. Idempotence is judged on
    -- this, never on a filename or a modification time: a touched file is not
    -- a changed one, and a restored backup is not an unchanged one.
    source_fingerprint  TEXT        NOT NULL,
    -- The extractor's identity **as data**. No enum, no lookup table: adding a
    -- worker (PDF, OCR, LLM) must be a deployment rather than a migration, and
    -- a CHECK constraint listing known extractors would make it one.
    extractor           TEXT        NOT NULL,
    extractor_version   TEXT        NOT NULL,
    -- The full text as the parser produced it. Kept because decision 5 makes a
    -- claim without its source unverifiable, and because every evidence span
    -- below is an offset into *this* string — resolving them against a
    -- re-read of the original file would silently drift the moment anyone
    -- edits it.
    source_text         TEXT        NOT NULL,
    media_type          TEXT        NOT NULL,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    asserted            INTEGER     NOT NULL DEFAULT 0,
    surfaced            INTEGER     NOT NULL DEFAULT 0,
    discarded           INTEGER     NOT NULL DEFAULT 0
);

-- The idempotence lookup: "has this exact document already been through this
-- exact extractor?" All three columns, because any one alone is wrong — a
-- better extractor should re-read old documents, and an edited document should
-- be re-read by the same extractor.
CREATE INDEX extraction_runs_identity
    ON extraction_runs (source_id, source_fingerprint, extractor, extractor_version);

CREATE TABLE extraction_claims (
    id                UUID PRIMARY KEY,
    -- ON DELETE CASCADE is what makes "deletable wholesale" true rather than
    -- aspirational. A run deleted while its claims survived would leave
    -- unattributable assertions in graph:extraction — worse than the bad run,
    -- because nothing then records where they came from.
    run_id            UUID        NOT NULL REFERENCES extraction_runs(id) ON DELETE CASCADE,
    subject           TEXT        NOT NULL,
    predicate         TEXT        NOT NULL,
    object            TEXT        NOT NULL,
    confidence        DOUBLE PRECISION NOT NULL,
    evidence_start    INTEGER     NOT NULL,
    evidence_end      INTEGER     NOT NULL,
    -- 'asserted' | 'pending' | 'confirmed' | 'rejected'. A CHECK rather than
    -- an enum type so the states stay readable in psql and alterable without a
    -- type rewrite; the set is closed and owned by this codebase, unlike the
    -- extractor names above.
    --
    -- **'asserted' and 'confirmed' are deliberately not the same state.** One
    -- means the confidence band was high enough that no human was asked; the
    -- other means a human was asked and said yes. Collapsing them would record
    -- every machine assertion as human-reviewed, which is the precise
    -- provenance lie this epic exists to avoid.
    state             TEXT        NOT NULL DEFAULT 'pending'
                      CHECK (state IN ('asserted', 'pending', 'confirmed', 'rejected')),
    queued_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at        TIMESTAMPTZ,
    decided_by        TEXT
);

CREATE INDEX extraction_claims_pending
    ON extraction_claims (state, queued_at)
    WHERE state = 'pending';

-- **A rejection is looked up by what it says, not by which run said it.**
-- Decision: re-ingesting a document must not re-queue a claim a human already
-- rejected, and the second run has a different id — so the identity that
-- matters is the assertion itself.
CREATE INDEX extraction_claims_assertion
    ON extraction_claims (subject, predicate, object);

CREATE TABLE extraction_discards (
    id          UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES extraction_runs(id) ON DELETE CASCADE,
    subject     TEXT NOT NULL,
    predicate   TEXT NOT NULL,
    object      TEXT NOT NULL,
    confidence  DOUBLE PRECISION NOT NULL,
    -- Decision 1 requires discards to carry a reason. A run that quietly
    -- dropped half its output is indistinguishable from one that found
    -- nothing, and the difference is exactly what a mis-prompted extractor
    -- looks like.
    reason      TEXT NOT NULL
);
