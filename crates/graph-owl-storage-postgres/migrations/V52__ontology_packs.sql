-- Epic 33: domain ontology packs. A pack is a versioned, imported
-- vocabulary — never vendored into this repo (decision 1) — that lands as
-- Approved terms in its own glossary (decision 4), so `glossaries` /
-- `glossary_terms` (V20) are reused wholesale rather than duplicated.
--
-- Two tables beyond the pack record itself:
--   pack_terms:     which glossary_terms row came from which pack, and at
--                    which source concept IRI — the stable key an override
--                    or an upgrade addresses a term by, since a local id is
--                    not something the publisher promises to keep meaning
--                    anything across a re-import.
--   pack_overrides: an organization's local customization (decision 2,
--                    "extend without fork") — stored apart from pack
--                    content on purpose, so upgrading a pack (replacing its
--                    row and its pack_terms) never touches this table and
--                    an override survives by construction, not by care
--                    taken at upgrade time.
CREATE TABLE ontology_packs (
    id             UUID PRIMARY KEY,
    pack_id        TEXT NOT NULL CHECK (pack_id <> ''),
    version        TEXT NOT NULL CHECK (version <> ''),

    licence_kind    TEXT NOT NULL
                    CHECK (licence_kind IN ('permissive', 'attributionRequired', 'licenceRequired')),
    licence_name    TEXT NOT NULL CHECK (licence_name <> ''),
    -- Set only when licence_kind = 'attributionRequired'.
    licence_notice  TEXT,
    -- Set only when licence_kind = 'licenceRequired'.
    licence_contact TEXT,

    source_url   TEXT NOT NULL CHECK (source_url <> ''),
    glossary_id  UUID NOT NULL REFERENCES glossaries (id) ON DELETE CASCADE,
    term_count   INTEGER NOT NULL,
    imported_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- The exact bytes last imported — Slice D's upgrade diff re-parses this
    -- rather than reconstructing "what was installed" from term rows, which
    -- would only ever be as complete as whatever fields Slice D bothered to
    -- write back. This is the *declared* state; the term rows are the
    -- *applied* one — the same split Epic 20's drift model already uses.
    source_turtle BYTEA NOT NULL,

    -- Slice A's idempotency: re-importing the same version is a no-op,
    -- checked here rather than per-term — the whole pack either landed at
    -- this version already or it did not.
    UNIQUE (pack_id, version)
);

CREATE TABLE pack_terms (
    pack_id    UUID NOT NULL REFERENCES ontology_packs (id) ON DELETE CASCADE,
    term_id    UUID NOT NULL REFERENCES glossary_terms (id) ON DELETE CASCADE,
    source_iri TEXT NOT NULL CHECK (source_iri <> ''),
    PRIMARY KEY (pack_id, source_iri)
);

CREATE INDEX pack_terms_by_term ON pack_terms (term_id);

CREATE TABLE pack_overrides (
    id         UUID PRIMARY KEY,
    pack_id    UUID NOT NULL REFERENCES ontology_packs (id) ON DELETE CASCADE,
    -- The term's source_iri (pack_terms.source_iri) — not term_id, so an
    -- override still finds its target after an upgrade replaces the term
    -- row the IRI used to map to.
    term_path  TEXT NOT NULL CHECK (term_path <> ''),
    kind       TEXT NOT NULL CHECK (kind IN ('redefine', 'hide', 'addSynonym', 'addRelation')),
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX pack_overrides_by_pack_term ON pack_overrides (pack_id, term_path);
