-- Domains and data products — Epic 23.
--
-- **A second grouping axis, deliberately not the containment hierarchy.** The
-- technical hierarchy (service → database → schema → table) says where data
-- *lives*; a domain says who is *accountable* for it, and a data product says
-- what is *consumable*. One domain spans several services and one product
-- bundles assets from several schemas, so forcing either into `assets.parent_id`
-- would make one of the three wrong.

CREATE TABLE domains (
    id                   UUID PRIMARY KEY,
    name                 TEXT NOT NULL,
    -- Derived from the parent chain the way an asset's is, never client-set:
    -- `payments.billing` is a *path*, and letting a client supply it makes the
    -- path and the parent able to disagree.
    fully_qualified_name TEXT NOT NULL UNIQUE,

    -- **A self-referential nullable foreign key, not a join table.** A domain has
    -- at most one parent — that is what makes it a hierarchy rather than a graph
    -- — and a join table would make two parents representable, which the cycle
    -- check would then have to defend against on every read.
    --
    -- `ON DELETE RESTRICT` so a deleted parent cannot silently orphan its
    -- children into roots. Slice F's job is to make that refusal visible with a
    -- count, and `SET NULL` here would take the decision away from it.
    parent_id            UUID REFERENCES domains (id) ON DELETE RESTRICT,

    description          TEXT,
    -- Open text, not an enum: "source-aligned", "consumer-aligned" and
    -- "aggregate" are one framework's vocabulary, and an organization that uses
    -- another should not need a migration to say so.
    domain_type          TEXT,

    -- The envelope, the same one Epic 3 put on assets. A domain is an entity
    -- somebody owns and edits, so "who changed this and when" has to be
    -- answerable about it too.
    version_major        INT NOT NULL DEFAULT 0,
    version_minor        INT NOT NULL DEFAULT 1,
    updated_by           TEXT NOT NULL DEFAULT 'system',
    change_description   JSONB,
    deleted              BOOLEAN NOT NULL DEFAULT FALSE,
    deleted_at           TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- The depth-1 cycle, refused by the database rather than only by the
    -- application. It is the one a careless `UPDATE` creates and the cheapest
    -- possible check; deeper cycles need the ancestor walk and live in the
    -- adapter, because SQL cannot express "would this edge close a loop" as a
    -- constraint.
    CONSTRAINT domains_no_self_parent CHECK (id <> parent_id)
);

CREATE INDEX domains_by_parent ON domains (parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX domains_live ON domains (fully_qualified_name) WHERE NOT deleted;

-- Named people to ask, distinct from owners.
--
-- **Not the same relation as ownership** (Epic 11): an owner is accountable and
-- an expert is knowledgeable, and conflating them means either the accountable
-- person is presumed to know the data or the knowledgeable one is presumed
-- answerable for it. Both are wrong often enough to matter.
CREATE TABLE domain_experts (
    domain_id UUID NOT NULL REFERENCES domains (id) ON DELETE CASCADE,
    user_id   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    ordinal   INT  NOT NULL DEFAULT 0,
    PRIMARY KEY (domain_id, user_id)
);

CREATE INDEX domain_experts_by_user ON domain_experts (user_id);

-- **One column, because decision 1 is "at most one domain".** A join table
-- would make two assignments representable, and every read would then have to
-- decide which one wins — which is precisely the shared accountability the
-- decision refuses. Exclusivity that the schema cannot express is exclusivity
-- that eventually is not true.
--
-- `ON DELETE RESTRICT`: deleting a domain that still holds assets is Slice F's
-- guarded operation, not a silent unassignment.
ALTER TABLE assets ADD COLUMN domain_id UUID REFERENCES domains (id) ON DELETE RESTRICT;

-- Partial: most assets inherit rather than being assigned, so indexing the
-- nulls would be indexing the majority for a query nobody runs.
CREATE INDEX assets_by_domain ON assets (domain_id) WHERE domain_id IS NOT NULL;

CREATE TABLE data_products (
    id                   UUID PRIMARY KEY,
    name                 TEXT NOT NULL,
    fully_qualified_name TEXT NOT NULL UNIQUE,
    description          TEXT,
    -- What it is *for*, separate from what it *is*. A product with no stated
    -- purpose is the failure this entity exists to prevent — a bundle of tables
    -- somebody assembled and nobody can explain.
    purpose              TEXT,
    -- Exactly one domain: a product is a consumable unit *of* an accountable
    -- group, and one owned by two groups is owned by neither. Nullable so a
    -- product can be drafted before its domain is decided.
    domain_id            UUID REFERENCES domains (id) ON DELETE RESTRICT,

    version_major        INT NOT NULL DEFAULT 0,
    version_minor        INT NOT NULL DEFAULT 1,
    updated_by           TEXT NOT NULL DEFAULT 'system',
    change_description   JSONB,
    deleted              BOOLEAN NOT NULL DEFAULT FALSE,
    deleted_at           TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX data_products_by_domain ON data_products (domain_id) WHERE domain_id IS NOT NULL;

-- **Many-to-many, which is the inverse of the domain rule and easy to get
-- wrong by copying it.** An asset belongs to any number of products: the same
-- orders table can be in "Customer 360" and in "Finance Reporting", and that is
-- not a governance failure — those are two consumable views of one thing.
--
-- `ON DELETE CASCADE` on both sides: removing an asset from a product is not a
-- delete of either, so the edge is the only thing that ever needs removing.
CREATE TABLE data_product_assets (
    data_product_id UUID NOT NULL REFERENCES data_products (id) ON DELETE CASCADE,
    asset_id        UUID NOT NULL REFERENCES assets (id) ON DELETE CASCADE,
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (data_product_id, asset_id)
);

-- Read in both directions: "what is in this product" on a product page, and
-- "which products contain this asset" on an asset page.
CREATE INDEX data_product_assets_by_asset ON data_product_assets (asset_id);

CREATE TABLE domain_versions (
    domain_id          UUID NOT NULL REFERENCES domains (id) ON DELETE CASCADE,
    version_major      INT NOT NULL,
    version_minor      INT NOT NULL,
    snapshot           JSONB NOT NULL,
    change_description JSONB,
    updated_by         TEXT NOT NULL,
    updated_at         TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (domain_id, version_major, version_minor)
);

CREATE TABLE data_product_versions (
    data_product_id    UUID NOT NULL REFERENCES data_products (id) ON DELETE CASCADE,
    version_major      INT NOT NULL,
    version_minor      INT NOT NULL,
    snapshot           JSONB NOT NULL,
    change_description JSONB,
    updated_by         TEXT NOT NULL,
    updated_at         TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (data_product_id, version_major, version_minor)
);
