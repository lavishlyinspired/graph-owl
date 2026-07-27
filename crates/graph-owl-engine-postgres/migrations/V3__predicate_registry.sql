-- Predicates definable at runtime, so an organisation can extend the
-- vocabulary without a release.
CREATE TABLE predicates (
    namespace   INTEGER NOT NULL CHECK (namespace BETWEEN 0 AND 65535),
    name        TEXT    NOT NULL,

    -- Which FlakeValue variant this predicate's objects must be. See
    -- graph_owl_core::flake::value_type.
    value_type  SMALLINT NOT NULL CHECK (value_type BETWEEN 0 AND 9),

    -- FALSE = at most one value per subject; TRUE = many.
    --
    -- Cardinality is a property of the predicate, not of the writer: `dsc:name`
    -- is single-valued for everyone, and leaving it to each caller means the
    -- first one that forgets makes the graph have two names for one table with
    -- nothing to say which is current.
    many        BOOLEAN NOT NULL DEFAULT FALSE,

    -- Core predicates ship with the binary and cannot be redefined at runtime.
    -- Redefining `dsc:fqn` from a string to a reference would not migrate the
    -- flakes already written against it -- it would just make every one of them
    -- unreadable, silently.
    core        BOOLEAN NOT NULL DEFAULT FALSE,

    PRIMARY KEY (namespace, name)
);

-- The core vocabulary, seeded here rather than inserted by application startup:
-- a definition the application writes is one an application bug can rewrite,
-- and these are the predicates every stored flake already depends on.
--
-- value_type: 0=ref 1=str 2=bool 3=int 4=float 5=instant 6=json
-- namespace 1 = dsc:
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    -- envelope
    (1, 'type',            1, FALSE, TRUE),
    (1, 'name',            1, FALSE, TRUE),
    (1, 'displayName',     1, FALSE, TRUE),
    (1, 'fqn',             1, FALSE, TRUE),
    (1, 'description',     1, FALSE, TRUE),
    (1, 'version',         1, FALSE, TRUE),
    (1, 'createdAt',       5, FALSE, TRUE),
    (1, 'updatedAt',       5, FALSE, TRUE),
    (1, 'updatedBy',       1, FALSE, TRUE),
    (1, 'deleted',         2, FALSE, TRUE),
    (1, 'deletedAt',       5, FALSE, TRUE),
    (1, 'properties',      6, FALSE, TRUE),
    -- many-valued by nature: an asset has any number of these
    (1, 'owner',           0, TRUE,  TRUE),
    (1, 'tag',             1, TRUE,  TRUE),
    (1, 'domain',          0, TRUE,  TRUE),
    (1, 'lifecycle',       1, FALSE, TRUE),
    (1, 'extension',       6, FALSE, TRUE),
    -- structural
    (1, 'parentService',   0, FALSE, TRUE),
    (1, 'parentDatabase',  0, FALSE, TRUE),
    (1, 'parentSchema',    0, FALSE, TRUE),
    (1, 'parentTable',     0, FALSE, TRUE),
    (1, 'dataType',        1, FALSE, TRUE),
    (1, 'ordinalPosition', 3, FALSE, TRUE),
    (1, 'nullable',        2, FALSE, TRUE),
    -- relationships, reified
    (1, 'fromEntity',      0, FALSE, TRUE),
    (1, 'toEntity',        0, FALSE, TRUE),
    (1, 'relType',         1, FALSE, TRUE),
    (1, 'fromEntityType',  1, FALSE, TRUE),
    (1, 'toEntityType',    1, FALSE, TRUE),
    -- provenance
    (1, 'sourceType',      1, FALSE, TRUE),
    (1, 'sourceUrl',       1, FALSE, TRUE),
    (1, 'confidence',      4, FALSE, TRUE),
    (1, 'lastVerifiedAt',  5, FALSE, TRUE);

-- rdf:type, namespace 256.
INSERT INTO predicates (namespace, name, value_type, many, core) VALUES
    (256, 'type', 0, TRUE, TRUE);
