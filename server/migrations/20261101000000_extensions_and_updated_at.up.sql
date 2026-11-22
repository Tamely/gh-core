-- Core schema, step 1. These core-schema migrations (20261101…) run before every other
-- gh-core migration; later PRs use later timestamps.

-- pgvector 0.8+ is required: iterative index scans (hnsw.iterative_scan) arrived in 0.8.
-- Always in public, so per-schema test setups (search_path = test, public) share one copy.
CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;
DO $$
BEGIN
    IF (SELECT string_to_array(extversion, '.')::int[] FROM pg_extension WHERE extname = 'vector') < ARRAY[0, 8] THEN
        RAISE EXCEPTION 'gh-core needs pgvector 0.8 or newer (ALTER EXTENSION vector UPDATE)';
    END IF;
END
$$;

-- Shared trigger: keeps updated_at current on every UPDATE.
CREATE FUNCTION set_updated_at() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END
$$;
