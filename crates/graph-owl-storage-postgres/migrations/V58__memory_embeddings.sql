-- Epic 31's semantic ranking term. One row per memory that has been
-- embedded — not every memory has one, by design: a memory written before
-- this feature existed, or whose embedding call failed, is simply absent
-- (Storage::memory_embeddings' own contract), never a zero vector.
--
-- REAL[] rather than pgvector's `vector` type: this table backs a rerank
-- over an already-filtered candidate set (memories about one subject),
-- never an approximate-nearest-neighbour search over the whole corpus —
-- graph_owl_search::embeddings::cosine_similarity runs in Rust over a
-- handful of rows per recall, so there is nothing here for an ANN index to
-- speed up, and no reason to add the extension.
CREATE TABLE memory_embeddings (
    memory_id  UUID PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    embedding  REAL[] NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
