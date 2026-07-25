CREATE TABLE tables (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL CHECK (name <> ''),
    fully_qualified_name TEXT NOT NULL UNIQUE CHECK (fully_qualified_name <> ''),
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
