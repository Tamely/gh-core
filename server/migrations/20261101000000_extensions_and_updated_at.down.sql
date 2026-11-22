DROP FUNCTION set_updated_at();
-- The up only creates vector if it is missing, and per-schema test setups share the one
-- copy in public, so drop it only when no table anywhere still has a vector column.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_attribute WHERE atttypid = 'vector'::regtype AND NOT attisdropped) THEN
        DROP EXTENSION vector;
    END IF;
END
$$;
