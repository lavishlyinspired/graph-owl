-- Data contracts — Epic 27.
--
-- **A contract is an entity with parties** (decision 1), not an annotation on a
-- table. It has a producer, consumers, an owner, a version and a lifecycle, all
-- of which need an envelope — and several contracts may exist on one asset,
-- because different consumers agree to different things.

CREATE TABLE contracts (
    id                 UUID PRIMARY KEY,
    name               TEXT NOT NULL CHECK (name <> ''),

    -- The asset the promise is about. FQN rather than an id, matching every
    -- other cross-entity reference added since Epic 24: a column is addressed
    -- that way and is contractable.
    asset_fqn          TEXT NOT NULL CHECK (asset_fqn <> ''),

    -- The team that owns the asset and makes the promise. `RESTRICT`, because a
    -- contract whose producer vanished is a promise nobody is accountable for —
    -- and Epic 11's principal deletion already knows how to refuse or reassign.
    producer           TEXT NOT NULL REFERENCES teams (id) ON DELETE RESTRICT,

    -- Avro's names. *Backward* means a new reader can read old data; *forward*
    -- means an old reader can read new data. Swapping them is the classic
    -- error, which is why `graph_owl_core::contract` writes the matrix out cell
    -- by cell rather than deriving it.
    compatibility      TEXT NOT NULL DEFAULT 'none'
                       CHECK (compatibility IN ('none', 'backward', 'forward', 'full')),

    status             TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft', 'active', 'violated', 'terminated')),

    -- **`allow_additional` is separate from the mode and overrides it.** A
    -- consumer reading `SELECT *` into a fixed struct breaks on any new column
    -- however nullable, so a contract may forbid additions outright — and that
    -- refusal has to beat a lenient mode or the flag would mean nothing.
    allow_additional   BOOLEAN NOT NULL DEFAULT TRUE,

    version_major      INT NOT NULL DEFAULT 0,
    version_minor      INT NOT NULL DEFAULT 1,
    updated_by         TEXT NOT NULL DEFAULT 'system',
    change_description JSONB,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Several contracts per asset is the realistic case, not the exception, so this
-- is deliberately **not** unique on `asset_fqn`.
CREATE INDEX contracts_by_asset ON contracts (asset_fqn) WHERE status IN ('active', 'violated');

CREATE TABLE contract_consumers (
    contract_id UUID NOT NULL REFERENCES contracts (id) ON DELETE CASCADE,
    team_id     TEXT NOT NULL REFERENCES teams (id) ON DELETE RESTRICT,
    PRIMARY KEY (contract_id, team_id)
);

CREATE INDEX contract_consumers_by_team ON contract_consumers (team_id);

-- The columns the contract promises will be there.
--
-- Rows rather than one JSON blob, because "which contracts guarantee this
-- column" is a question a producer asks before changing it — and that is an
-- index lookup against a column, not a scan through documents.
CREATE TABLE contract_columns (
    contract_id UUID NOT NULL REFERENCES contracts (id) ON DELETE CASCADE,
    name        TEXT NOT NULL CHECK (name <> ''),
    data_type   TEXT NOT NULL,
    nullable    BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (contract_id, name)
);

CREATE INDEX contract_columns_by_name ON contract_columns (name);

-- Promises about behaviour rather than shape. One JSON document per SLA because
-- the variants carry different fields, and a table with a nullable column per
-- variant is a table where most cells are null and no constraint means anything.
CREATE TABLE contract_slas (
    id          UUID PRIMARY KEY,
    contract_id UUID NOT NULL REFERENCES contracts (id) ON DELETE CASCADE,
    definition  JSONB NOT NULL
);

CREATE INDEX contract_slas_by_contract ON contract_slas (contract_id);

-- **Breaches accumulate; they are never cleared by a later compatible change.**
-- The incident happened, and a contract that forgot it would let a producer
-- break something on Monday and look clean on Tuesday. Clearing is explicit and
-- is a separate act, which is why there is no `resolved` column here to be
-- flipped by the evaluation path.
CREATE TABLE contract_breaches (
    id            UUID PRIMARY KEY,
    contract_id   UUID NOT NULL REFERENCES contracts (id) ON DELETE CASCADE,
    column_name   TEXT NOT NULL,
    detail        TEXT NOT NULL,
    -- The asset version that caused it, so "when did this break" is answerable
    -- against the asset's own history rather than only against a timestamp.
    asset_version TEXT NOT NULL,
    detected_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX contract_breaches_by_contract ON contract_breaches (contract_id, detected_at DESC);
