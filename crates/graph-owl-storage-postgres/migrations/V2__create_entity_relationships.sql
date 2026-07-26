CREATE TABLE entity_relationships (
    id UUID PRIMARY KEY,
    from_entity_type TEXT NOT NULL CHECK (from_entity_type <> ''),
    from_entity_id UUID NOT NULL,
    relationship_type TEXT NOT NULL CHECK (relationship_type <> ''),
    to_entity_type TEXT NOT NULL CHECK (to_entity_type <> ''),
    to_entity_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id)
);
