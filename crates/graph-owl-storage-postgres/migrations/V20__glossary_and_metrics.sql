-- Epic 24: business meaning — a glossary with a review workflow, SKOS relations,
-- and `Metric` as a first-class entity.
--
-- The rules `graph-owl-core::glossary` decides are enforced **again here** where
-- the database can enforce them, for the same reason V15 gives: the domain is
-- the only path an HTTP request takes, but not the only path a migration, a
-- repair script, or a second writer takes.

CREATE TABLE glossaries (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL CHECK (name <> ''),
    description  TEXT,
    -- The FQN root every term below this glossary derives from (Epic 2).
    fully_qualified_name TEXT NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE glossary_terms (
    id          UUID PRIMARY KEY,
    glossary_id UUID NOT NULL REFERENCES glossaries (id) ON DELETE RESTRICT,
    name        TEXT NOT NULL CHECK (name <> ''),

    -- `{glossary}.{term}` — derived, stored, and globally unique *as an FQN*.
    fully_qualified_name TEXT NOT NULL UNIQUE,

    definition  TEXT NOT NULL DEFAULT '',

    -- Slice C's workflow. The CHECK is the vocabulary; which *moves* are legal
    -- is `can_transition`'s decision and deliberately not encoded here — a
    -- transition matrix in a CHECK constraint cannot see the previous value
    -- without a trigger, and a trigger is a second place for the rule to live.
    status      TEXT NOT NULL DEFAULT 'draft'
                CHECK (status IN ('draft', 'inReview', 'approved', 'deprecated')),

    -- Slice C: deprecation carries a reason and an optional successor, so an
    -- asset still pointing at a dead term has somewhere to be sent.
    deprecation_reason TEXT,
    successor_term_id  UUID REFERENCES glossary_terms (id) ON DELETE SET NULL,

    -- Searchable per Slice A, which is why they are columns rather than a blob:
    -- the search vector below has to reach into them.
    synonyms      TEXT[] NOT NULL DEFAULT '{}',
    abbreviations TEXT[] NOT NULL DEFAULT '{}',

    version_major INTEGER NOT NULL DEFAULT 1,
    version_minor INTEGER NOT NULL DEFAULT 0,
    version_patch INTEGER NOT NULL DEFAULT 0,

    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- **Scoped uniqueness, not global** (Slice A). "Customer" means different
    -- things in a Finance glossary and a Support one, and forcing a single
    -- winner would make the second team invent "Customer (Support)" — which is
    -- the same collision with worse ergonomics.
    UNIQUE (glossary_id, name)
);

-- **`array_to_string` is STABLE, not IMMUTABLE, so a generated column cannot
-- call it** — Postgres refuses the whole `ALTER` with "generation expression is
-- not immutable". The reason the built-in is only stable is polymorphism: it
-- accepts `anyarray`, and some element types' output functions genuinely depend
-- on a GUC (`timestamptz` reads `TimeZone`).
--
-- Narrowed to `text[]`, that reason evaporates — `text`'s output function is the
-- identity — so this wrapper's IMMUTABLE claim is true rather than asserted. The
-- narrow signature is the point, and it is why this must not be widened to
-- `anyarray` later: that would restore exactly the dependency being escaped, and
-- the symptom would be a search index silently wrong for one session's GUC.
CREATE FUNCTION text_array_to_string(text[]) RETURNS text
    LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
    AS $$ SELECT array_to_string($1, ' ') $$;

-- Slice A: a synonym match must find the term, so synonyms and abbreviations are
-- in the indexed document. Weighted below the name: an exact name match should
-- outrank a term that merely lists the query as an abbreviation.
ALTER TABLE glossary_terms ADD COLUMN search_vector tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(name, '')), 'A')
        || setweight(to_tsvector('english', text_array_to_string(synonyms)), 'B')
        || setweight(to_tsvector('english', text_array_to_string(abbreviations)), 'B')
        || setweight(to_tsvector('english', coalesce(definition, '')), 'C')
    ) STORED;

CREATE INDEX glossary_terms_search ON glossary_terms USING GIN (search_vector);
CREATE INDEX glossary_terms_by_glossary ON glossary_terms (glossary_id);

-- **One row per relation, in the direction it was declared** (Slice B).
--
-- `narrower` is *derived on read* from the stored `broader`, never stored
-- alongside it. Two rows for one fact can disagree, and the one that is wrong is
-- whichever nobody looked at.
CREATE TABLE term_relations (
    term_id  UUID NOT NULL REFERENCES glossary_terms (id) ON DELETE CASCADE,
    kind     TEXT NOT NULL CHECK (kind IN ('broader', 'narrower', 'related', 'exactMatch', 'closeMatch')),

    -- A term id for the internal relations; an IRI for the match ones. One
    -- column because the alternative is two nullable columns plus a CHECK that
    -- exactly one is set, which stores the same thing with more ways to be
    -- inconsistent.
    target   TEXT NOT NULL CHECK (target <> ''),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (term_id, kind, target)
);

CREATE INDEX term_relations_by_target ON term_relations (target);

-- Slice C: approval requires an assigned reviewer, and only an assigned reviewer
-- may approve. A real foreign key, so a departed reviewer cannot go on appearing
-- to have authority.
CREATE TABLE term_reviewers (
    term_id UUID NOT NULL REFERENCES glossary_terms (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (term_id, user_id)
);

-- Slice C: who moved a term where, and when. Without it "approved" is a state
-- with no account of how it got there, which is the one thing a reviewer is for.
CREATE TABLE term_transitions (
    id          UUID PRIMARY KEY,
    term_id     UUID NOT NULL REFERENCES glossary_terms (id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    actor       TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    reason      TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX term_transitions_by_term ON term_transitions (term_id, occurred_at DESC);

-- Slice D: terms attach to assets and to individual columns.
CREATE TABLE term_attachments (
    term_id    UUID NOT NULL REFERENCES glossary_terms (id) ON DELETE CASCADE,
    -- FQN rather than an id, because a column is addressed by FQN and has no row
    -- of its own until Epic 22.
    target_fqn TEXT NOT NULL CHECK (target_fqn <> ''),
    attached_by TEXT NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    attached_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (term_id, target_fqn)
);

CREATE INDEX term_attachments_by_target ON term_attachments (target_fqn);

CREATE TABLE metrics (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL CHECK (name <> ''),

    -- **Namespaced away from tables** (Slice E). A metric called `revenue` and a
    -- table called `revenue` are different things, and a shared FQN space would
    -- make one of them unaddressable.
    fully_qualified_name TEXT NOT NULL UNIQUE,

    -- Required. A metric whose definition is blank is a name, and a name is what
    -- the ambiguity was in the first place.
    definition  TEXT NOT NULL CHECK (definition <> ''),

    -- **Prose, never evaluated** (decision 3). Storing an AST would imply a
    -- computation engine this deliberately is not.
    formula     TEXT,
    unit        TEXT,
    granularity TEXT,

    calculation_type TEXT NOT NULL DEFAULT 'simple'
        CHECK (calculation_type IN ('simple', 'ratio', 'derived', 'composite')),

    -- Slice E: must reference an *approved* term. Enforced in the domain, where
    -- the status can be read; the FK here only guarantees the term exists.
    defined_by  UUID REFERENCES glossary_terms (id) ON DELETE SET NULL,

    version_major INTEGER NOT NULL DEFAULT 1,
    version_minor INTEGER NOT NULL DEFAULT 0,
    version_patch INTEGER NOT NULL DEFAULT 0,

    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE metrics ADD COLUMN search_vector tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('english', coalesce(name, '')), 'A')
        || setweight(to_tsvector('english', coalesce(definition, '')), 'B')
    ) STORED;

CREATE INDEX metrics_search ON metrics USING GIN (search_vector);

-- Slice F: the sources a metric declares. Lineage edges are **derived** from
-- these rather than asserted separately (decision 5) — requiring both invites
-- the two to diverge, and then neither is trustworthy.
CREATE TABLE metric_sources (
    metric_id  UUID NOT NULL REFERENCES metrics (id) ON DELETE CASCADE,
    source_fqn TEXT NOT NULL CHECK (source_fqn <> ''),
    PRIMARY KEY (metric_id, source_fqn)
);

CREATE INDEX metric_sources_by_source ON metric_sources (source_fqn);
