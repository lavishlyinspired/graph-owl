-- Epic 31: organizational memory — the knowledge that otherwise evaporates
-- into chats, tickets and notebooks.
--
-- Three invariants the domain already enforces are enforced **again here**, on
-- purpose. `graph-owl-core::memory` is the only path an HTTP request can take,
-- but it is not the only path a migration, a repair script, or a future second
-- writer can take — and every one of those is how a trust signal actually gets
-- corrupted in a live system.
CREATE TABLE memories (
    id      UUID PRIMARY KEY,
    kind    TEXT NOT NULL CHECK (kind IN ('rationale', 'incident', 'decision', 'caveat')),
    content TEXT NOT NULL CHECK (content <> ''),

    -- Nullable: a forced summary is a truncated first sentence, which is worse
    -- than none.
    summary TEXT,

    -- **Authorship, as discriminated columns rather than a JSON blob.**
    --
    -- Same argument as V13's owner columns: `author_user_id` is a real foreign
    -- key, so a deleted person cannot go on being cited as the human who stood
    -- behind a claim. A JSON `{"kind":"human","userId":...}` cannot reference
    -- anything, and the whole point of recording a human author is that somebody
    -- can be asked.
    --
    -- Agent identity is two columns because *which* agent matters when its
    -- conclusions turn out wrong and somebody has to find the rest of them —
    -- and the model matters because that is what changed underneath.
    author_kind     TEXT NOT NULL CHECK (author_kind IN ('human', 'agent')),
    author_user_id  TEXT REFERENCES users (id) ON DELETE SET NULL,
    author_agent_id TEXT,
    author_model    TEXT,

    -- Exactly one shape, and it must match the discriminant. Without this a row
    -- can claim `human` while carrying an agent id, which is precisely the
    -- relabelling the domain refuses to let a PATCH do — refusing it in one
    -- layer only means refusing it on one path only.
    --
    -- `author_user_id` is deliberately *not* required to stay non-null: the FK
    -- above sets it null when a person is deleted, and losing the memory would
    -- be worse than losing the attribution. So the constraint pins the agent
    -- side, which nothing else can null out.
    CONSTRAINT memories_authorship_shape CHECK (
        (author_kind = 'human' AND author_agent_id IS NULL AND author_model IS NULL)
        OR
        (author_kind = 'agent' AND author_agent_id IS NOT NULL
                              AND author_model IS NOT NULL
                              AND author_user_id IS NULL)
    ),

    confidence DOUBLE PRECISION NOT NULL CHECK (confidence BETWEEN 0 AND 1),

    -- The instant this was true of its subject. Compared against the subject's
    -- current version to compute staleness on read — **there is deliberately no
    -- staleness column**. Whether a memory still describes its subject changes
    -- when the *subject* changes, so a stored flag is wrong the moment somebody
    -- edits the table, and wrong silently.
    as_of TIMESTAMPTZ NOT NULL,

    -- Correction, as two halves of one relationship. Both nullable, both real
    -- foreign keys, so a chain cannot point at a memory that no longer exists.
    --
    -- `ON DELETE RESTRICT` rather than CASCADE or SET NULL: deleting a memory
    -- somebody corrected would destroy the record of what people believed
    -- before they were corrected, which is most of the reason to keep a record
    -- at all. Refusing the delete is the honest failure.
    supersedes    UUID UNIQUE REFERENCES memories (id) ON DELETE RESTRICT,
    superseded_by UUID UNIQUE REFERENCES memories (id) ON DELETE RESTRICT,

    -- A memory cannot correct itself. Cheap to state, and the alternative is a
    -- one-element cycle that makes chain traversal non-terminating.
    CONSTRAINT memories_no_self_supersession CHECK (
        id <> supersedes AND id <> superseded_by
    ),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Retrieval is "what do we know about this", so the default read is "current
-- memories only" and the index has to serve it.
CREATE INDEX memories_current ON memories (as_of DESC) WHERE superseded_by IS NULL;

-- Links, with **referential integrity preserved by splitting the target**.
--
-- A link points at either a catalog asset or another memory. A single
-- polymorphic `target UUID` column could not be a foreign key, and V13 already
-- refused that trade: losing referential integrity to save a column lets a
-- deleted asset stay named as a subject, which is exactly the silent rot this
-- schema refuses everywhere else. `graph-owl-core`'s `MemoryLink` carries one
-- `Uuid` because the domain does not care which it is; the adapter resolves it,
-- and the resolution is needed anyway to answer Slice A's "a link to a
-- nonexistent target → 400".
CREATE TABLE memory_links (
    memory_id UUID NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    relation  TEXT NOT NULL CHECK (
        relation IN ('about', 'affects', 'evidence', 'follows', 'contradicts', 'mentions')
    ),

    asset_target  UUID REFERENCES assets (id) ON DELETE CASCADE,
    memory_target UUID REFERENCES memories (id) ON DELETE CASCADE,

    CONSTRAINT memory_links_one_target CHECK (
        (asset_target IS NOT NULL AND memory_target IS NULL)
        OR
        (asset_target IS NULL AND memory_target IS NOT NULL)
    ),

    -- No link to itself: `Contradicts` pointing at its own memory is a data
    -- error that would put an unresolvable item in a review queue, and `About`
    -- pointing at itself is an anchor to nothing.
    CONSTRAINT memory_links_not_self CHECK (memory_id <> memory_target)
);

-- One edge per (memory, relation, target). Two identical links are not two
-- facts. Expressed as two partial unique indexes because the target lives in
-- whichever column applies, and a composite key over both would treat
-- `(x, NULL)` rows as distinct — NULLs are not equal in a unique index.
CREATE UNIQUE INDEX memory_links_asset_identity
    ON memory_links (memory_id, relation, asset_target) WHERE asset_target IS NOT NULL;
CREATE UNIQUE INDEX memory_links_memory_identity
    ON memory_links (memory_id, relation, memory_target) WHERE memory_target IS NOT NULL;

-- **The retrieval index.** "What do we know about this asset" is the query the
-- whole epic exists to answer, and it reads by target.
CREATE INDEX memory_links_by_asset ON memory_links (asset_target) WHERE asset_target IS NOT NULL;
CREATE INDEX memory_links_by_memory ON memory_links (memory_target) WHERE memory_target IS NOT NULL;

-- A pair a human has reviewed, and what they decided.
--
-- **One table with a verdict, not a dismissals table beside a confirmations
-- table.** Two tables would make "confirmed *and* dismissed" representable, and
-- changing one's mind would be a delete from one plus an insert into the other
-- with nothing making the pair atomic. One row per pair makes the contradictory
-- state unrepresentable and a change of mind a plain `UPDATE`.
--
-- Reviewing is **not resolving**. A confirmed pair stays in the queue flagged as
-- confirmed; only a dismissal removes it. Software adjudicating institutional
-- disagreement is worse than the disagreement.
CREATE TABLE memory_contradiction_reviews (
    -- **Normalised, and the database enforces it.** `a < b` makes the unordered
    -- pair a schema guarantee rather than a Rust convention: a verdict recorded
    -- as `(b, a)` would silently stop applying, the queue would quietly reopen or
    -- downgrade the pair, and it would be impossible to reproduce on demand
    -- because it depends on load order.
    a UUID NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    b UUID NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    CONSTRAINT memory_reviews_normalised CHECK (a < b),

    verdict TEXT NOT NULL CHECK (verdict IN ('confirmed', 'dismissed')),

    -- Who, and why. A verdict with no author is an unattributable judgement about
    -- institutional disagreement, which is the one thing this epic must never
    -- produce.
    reviewed_by TEXT NOT NULL REFERENCES users (id),
    reviewed_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Nullable: "these are about different quarters" is worth capturing, and
    -- forcing a note gets the field filled with "n/a".
    note TEXT,

    PRIMARY KEY (a, b)
);

-- **`system` becomes an addressable user, so machine actions stay attributable.**
--
-- `dismissed_by` above is a real foreign key, on the principle that a dismissal
-- with no author is an unattributable judgement about institutional
-- disagreement — the one thing this epic must never produce. But `system` is a
-- real principal in this codebase (migrations, reconciliation, and every
-- unauthenticated request until Epic 12 lands) and had no row, so the constraint
-- turned every machine dismissal into a 500.
--
-- Seeding the row is the honest fix rather than dropping the constraint: a
-- dismissal by `system` then reads as visibly machine-made, which is the same
-- standard `Principal::system()` already documents for writes.
--
-- `is_admin` stays FALSE deliberately. This row exists for **attribution**, not
-- authorisation — the request-scoped principal is what grants admin, and a
-- stored admin row named `system` would be a standing privilege nobody
-- provisioned.
INSERT INTO users (id, display_name, is_admin, is_bot)
VALUES ('system', 'system', FALSE, TRUE)
ON CONFLICT (id) DO NOTHING;
