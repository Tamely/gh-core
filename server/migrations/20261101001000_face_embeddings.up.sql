-- One on-device face embedding per user (moved out of users). Deleting the user deletes it.
CREATE TABLE face_embeddings (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding vector(512) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER face_embeddings_updated_at BEFORE UPDATE ON face_embeddings
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX face_embeddings_embedding ON face_embeddings
    USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);

-- The vector search pattern (the facial recognition PR copies it for photo faces):
-- the HNSW index only speeds up ORDER BY <=> LIMIT, never a WHERE on the distance, so
-- take the top k first and only then keep distance <= 0.32 (cosine similarity >= 0.68).
-- iterative_scan keeps the index scanning past ef_search instead of stopping early.
-- With only a few thousand rows the planner prefers a sequential scan, so this function
-- turns that off; ef_search is tuned for recall >= 0.95 (server/db-tests checks both).
CREATE FUNCTION nearest_faces(query vector(512), k INT DEFAULT 50)
RETURNS TABLE (user_id UUID, distance DOUBLE PRECISION)
LANGUAGE sql STABLE STRICT -- k = NULL returns nothing, not every row
SET hnsw.iterative_scan = relaxed_order
SET hnsw.ef_search = 100
SET enable_seqscan = off
AS $$
    WITH nearest AS MATERIALIZED (
        SELECT f.user_id, f.embedding <=> query AS distance
        FROM face_embeddings f
        ORDER BY f.embedding <=> query
        LIMIT k
    )
    SELECT n.user_id, n.distance FROM nearest n WHERE n.distance <= 0.32 ORDER BY n.distance
$$;
