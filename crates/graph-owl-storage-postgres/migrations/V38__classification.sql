-- Tags and classifications — Epic 25.
--
-- **The operational vocabulary, separate from the semantic one.** Epic 24's
-- `glossary_terms` answers "what does this mean"; these answer "how do I handle
-- this". Decision 1 keeps them apart because collapsing them into one tree
-- would make every handling rule also a definition, and neither would be usable
-- as the other.

CREATE TABLE classifications (
    id                 UUID PRIMARY KEY,
    name               TEXT NOT NULL UNIQUE CHECK (name <> ''),
    description        TEXT,

    -- **Decision 4.** A `Tier` marked exclusive refuses a second `Tier.*` on
    -- one entity. Without it "Tier" means nothing: an entity could be Gold and
    -- Bronze at once and no consumer could act on either. Defaulting to FALSE
    -- because most vocabularies are additive — `PII.Sensitive` beside
    -- `PII.Restricted` is normal — and a default that refused the common case
    -- would be discovered only after somebody hit it.
    mutually_exclusive BOOLEAN NOT NULL DEFAULT FALSE,

    version_major      INT NOT NULL DEFAULT 0,
    version_minor      INT NOT NULL DEFAULT 1,
    updated_by         TEXT NOT NULL DEFAULT 'system',
    change_description JSONB,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tags (
    id                   UUID PRIMARY KEY,
    name                 TEXT NOT NULL CHECK (name <> ''),
    classification_id    UUID NOT NULL REFERENCES classifications (id) ON DELETE RESTRICT,
    -- `{classification}.{tag}`, derived. Unique globally because it is how a
    -- label refers to a tag — two tags resolving to one FQN would make every
    -- application ambiguous.
    fully_qualified_name TEXT NOT NULL UNIQUE,
    description          TEXT,

    version_major        INT NOT NULL DEFAULT 0,
    version_minor        INT NOT NULL DEFAULT 1,
    updated_by           TEXT NOT NULL DEFAULT 'system',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- **Scoped to the classification, not global.** `Gold` under `Tier` and
    -- `Gold` under `SupportPlan` are different tags; a global unique index on
    -- `name` would refuse the second, which is the mutation this constraint
    -- exists to make impossible rather than merely tested.
    UNIQUE (classification_id, name)
);

CREATE INDEX tags_by_classification ON tags (classification_id);

-- A tag applied to something.
--
-- **Keyed by target FQN, not by asset id.** A column is addressed by FQN and a
-- label has to be able to land on one — the same choice Epic 24's
-- `term_attachments` made, and for the same reason. It also means a label
-- survives an id change and is matched by *name* rather than position, which is
-- what Slice C's reorder criterion needs.
CREATE TABLE tag_labels (
    tag_id       UUID NOT NULL REFERENCES tags (id) ON DELETE RESTRICT,
    target_fqn   TEXT NOT NULL CHECK (target_fqn <> ''),

    -- Provenance from day one (decision 2). A scanner must be able to suggest
    -- without a human having confirmed, and a model that cannot say which is
    -- which forces a rewrite once automation arrives — with labelled data to
    -- migrate.
    label_type   TEXT NOT NULL DEFAULT 'manual'
                 CHECK (label_type IN ('manual', 'propagated', 'automated', 'derived')),
    state        TEXT NOT NULL DEFAULT 'confirmed'
                 CHECK (state IN ('suggested', 'confirmed')),

    applied_by   TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    applied_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    confirmed_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    confirmed_at TIMESTAMPTZ,

    -- **The primary key is the idempotency.** Applying the same tag to the same
    -- thing twice is one label, not two — the state the caller asked for rather
    -- than an error, the same rule Epic 11's follow endpoint follows.
    PRIMARY KEY (tag_id, target_fqn)
);

CREATE INDEX tag_labels_by_target ON tag_labels (target_fqn);
-- The steward triage queue reads this way: "what is waiting for me".
CREATE INDEX tag_labels_pending ON tag_labels (state) WHERE state = 'suggested';

-- **A rejection is kept, not deleted** — Slice D's sharp criterion. A rejection
-- that merely removed the label would be re-proposed by the next run of the
-- same scanner, and a steward would answer the same question forever. This is
-- the same rule Epic 21's `extraction_claims` rejection ledger follows, for the
-- same reason.
CREATE TABLE tag_rejections (
    tag_id      UUID NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    target_fqn  TEXT NOT NULL,
    rejected_by TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    rejected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tag_id, target_fqn)
);
