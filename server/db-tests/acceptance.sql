-- Acceptance checks that fit in one session. run.sh runs this on a migrated, seeded
-- database, then does the checks that need COMMIT or several connections.
-- Every check raises on failure; results are printed as NOTICEs.
\set ON_ERROR_STOP 1

-- Rules for every table: every foreign key picks CASCADE, RESTRICT or SET NULL (never the
-- default NO ACTION), and every foreign key's columns lead some index.
DO $$
DECLARE bad text;
BEGIN
    SELECT string_agg(conrelid::regclass || '.' || conname, ', ') INTO bad
    FROM pg_constraint WHERE contype = 'f' AND confdeltype = 'a';
    IF bad IS NOT NULL THEN RAISE EXCEPTION 'foreign keys without an explicit ON DELETE: %', bad; END IF;

    SELECT string_agg(c.conrelid::regclass || '.' || c.conname, ', ') INTO bad
    FROM pg_constraint c
    WHERE c.contype = 'f' AND NOT EXISTS (
        SELECT 1 FROM pg_index i
        WHERE i.indrelid = c.conrelid
          AND (i.indkey::int2[])[0:cardinality(c.conkey) - 1] = c.conkey);
    IF bad IS NOT NULL THEN RAISE EXCEPTION 'foreign keys without an index: %', bad; END IF;
    RAISE NOTICE 'ok: every foreign key has an ON DELETE rule and an index';
END
$$;

-- The shared trigger keeps updated_at current.
BEGIN;
DO $$
DECLARE t timestamptz;
BEGIN
    UPDATE teams SET updated_at = '2000-01-01', name = name || '!' WHERE name = 'Team 1' RETURNING updated_at INTO t;
    IF t < '2001-01-01' THEN RAISE EXCEPTION 'updated_at was not set by the trigger'; END IF;
    RAISE NOTICE 'ok: updated_at trigger';
END
$$;
ROLLBACK;

-- Duplicate check timing: 1,000 duplicate scans against the 5,000-row seeded table,
-- p99 of EXPLAIN ANALYZE execution time under 1ms. Runs before anything adds rows.
-- TIMING OFF everywhere: the total execution time is still measured, without the
-- per-plan-node clock reads that inflate it on small VMs.
DO $$
DECLARE r record; j json; times float8[] := '{}'; p99 float8;
BEGIN
    IF (SELECT count(*) FROM entitlement_scans) <> 5000 THEN
        RAISE EXCEPTION 'expected the 5,000 seeded scans, found %', (SELECT count(*) FROM entitlement_scans);
    END IF;
    FOR r IN SELECT user_id, entitlement_id FROM entitlement_scans ORDER BY random() LIMIT 1000 LOOP
        EXECUTE format('EXPLAIN (ANALYZE, TIMING OFF, FORMAT JSON) INSERT INTO entitlement_scans (user_id, entitlement_id, client_scanned_at) '
                       'VALUES (%L, %L, now()) ON CONFLICT DO NOTHING', r.user_id, r.entitlement_id) INTO j;
        times := times || (j -> 0 ->> 'Execution Time')::float8;
    END LOOP;
    SELECT percentile_disc(0.99) WITHIN GROUP (ORDER BY x) INTO p99 FROM unnest(times) x;
    IF (SELECT count(*) FROM entitlement_scans) <> 5000 THEN RAISE EXCEPTION 'a duplicate scan was inserted'; END IF;
    IF p99 >= 1 THEN RAISE EXCEPTION 'duplicate check p99 is % ms, budget is 1 ms', p99; END IF;
    RAISE NOTICE 'ok: duplicate check p99 % ms over 1,000 attempts (5,000 rows)', round(p99::numeric, 3);
END
$$;

-- Duplicate and unknown scans.
BEGIN;
DO $$
DECLARE u uuid; n int;
BEGIN
    SELECT user_id INTO u FROM entitlement_scans WHERE entitlement_id = 'shirt' LIMIT 1;
    BEGIN
        INSERT INTO entitlement_scans (user_id, entitlement_id, client_scanned_at) VALUES (u, 'shirt', now());
        RAISE EXCEPTION 'a plain INSERT of a duplicate scan was accepted';
    EXCEPTION WHEN unique_violation THEN NULL; -- 23505
    END;
    INSERT INTO entitlement_scans (user_id, entitlement_id, client_scanned_at) VALUES (u, 'shirt', now())
        ON CONFLICT DO NOTHING;
    GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 0 THEN RAISE EXCEPTION 'ON CONFLICT DO NOTHING inserted % rows', n; END IF;
    BEGIN
        INSERT INTO entitlement_scans (user_id, entitlement_id, client_scanned_at) VALUES (u, 'no-such-thing', now());
        RAISE EXCEPTION 'a scan of an unknown entitlement was accepted';
    EXCEPTION WHEN foreign_key_violation THEN NULL; -- 23503
    END;
    RAISE NOTICE 'ok: duplicate scan 23505, ON CONFLICT 0 rows, unknown entitlement rejected by the foreign key';
END
$$;
ROLLBACK;

-- Hardware: one open loan per asset; a new loan after the return works.
BEGIN;
DO $$
DECLARE a uuid; b uuid;
BEGIN
    SELECT id INTO a FROM users ORDER BY id LIMIT 1;
    SELECT id INTO b FROM users ORDER BY id OFFSET 1 LIMIT 1;
    INSERT INTO hardware_loans (asset_qr, user_id) VALUES ('ASSET-001', a);
    BEGIN
        INSERT INTO hardware_loans (asset_qr, user_id) VALUES ('ASSET-001', b);
        RAISE EXCEPTION 'an asset with an open loan was checked out again';
    EXCEPTION WHEN unique_violation THEN NULL;
    END;
    UPDATE hardware_loans SET returned_at = now() WHERE asset_qr = 'ASSET-001' AND returned_at IS NULL;
    INSERT INTO hardware_loans (asset_qr, user_id) VALUES ('ASSET-001', b);
    RAISE NOTICE 'ok: second checkout rejected, checkout after return accepted';
END
$$;
ROLLBACK;

-- Deleting a user deletes their face embedding.
BEGIN;
DO $$
DECLARE u uuid;
BEGIN
    SELECT user_id INTO u FROM face_embeddings LIMIT 1;
    DELETE FROM users WHERE id = u;
    IF EXISTS (SELECT 1 FROM face_embeddings WHERE user_id = u) THEN
        RAISE EXCEPTION 'face_embeddings row survived deleting its user';
    END IF;
    RAISE NOTICE 'ok: deleting a user removes their face_embeddings row';
END
$$;
ROLLBACK;

-- Status rules: a room's judging window is both ends or neither; a CLAIMED ticket has a
-- mentor; deleting the mentor puts their claimed ticket back in the open queue.
BEGIN;
DO $$
DECLARE m uuid; t uuid; st text;
BEGIN
    BEGIN
        INSERT INTO presentation_rooms (room_id, track, floor_number, svg_x, svg_y, judging_starts_at)
            VALUES ('HALF-WINDOW', 'GENERAL', 1, 0, 0, now());
        RAISE EXCEPTION 'a room with only a judging start time was accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        INSERT INTO mentor_tickets (location_label, svg_x, svg_y, tags, description, status, claimed_at)
            VALUES ('Table 1', 0, 0, '{}', 'help', 'CLAIMED', now());
        RAISE EXCEPTION 'a CLAIMED ticket with no mentor was accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    INSERT INTO users (email) VALUES ('mentor@example.invalid') RETURNING id INTO m;
    INSERT INTO mentor_tickets (location_label, svg_x, svg_y, tags, description, status, claimed_by, claimed_at)
        VALUES ('Table 1', 0, 0, '{}', 'help', 'CLAIMED', m, now()) RETURNING id INTO t;
    DELETE FROM users WHERE id = m;
    SELECT status INTO st FROM mentor_tickets WHERE id = t;
    IF st IS DISTINCT FROM 'OPEN' THEN RAISE EXCEPTION 'deleting the mentor left the ticket %', st; END IF;
    RAISE NOTICE 'ok: half judging window rejected, CLAIMED needs a mentor, deleting the mentor reopens the ticket';
END
$$;
ROLLBACK;

-- Vector search, with nearest_faces' own settings (iterative scan, ef_search, index
-- only) applied to this session. 100 query vectors: 50 noisy copies of seeded faces
-- with rising noise (cosine similarity to their source from about 0.77 down to 0.6, so
-- the 0.68 threshold splits them) and 50 random ones (no matches).
-- Latency is timed in its own loop; the exact scans for recall run afterwards, so they
-- don't evict the index from the CPU cache between timed queries.
SELECT count(set_config(split_part(c, '=', 1), split_part(c, '=', 2), false)) AS settings
FROM pg_proc, unnest(proconfig) c WHERE proname = 'nearest_faces' \gset
SELECT setseed(0.7) \gset
CREATE TEMP TABLE queries AS
    SELECT l2_normalize((SELECT embedding FROM face_embeddings ORDER BY user_id OFFSET i LIMIT 1)
                        + ARRAY(SELECT random_normal(0, 0.036 + 0.026 * i / 49) FROM generate_series(1, 512))::vector(512)) AS v
    FROM generate_series(0, 49) i
    UNION ALL
    SELECT l2_normalize(ARRAY(SELECT random_normal() FROM generate_series(1, 512) WHERE i IS NOT NULL)::vector(512))
    FROM generate_series(0, 49) i;
DO $$
DECLARE
    q record; j json; times float8[] := '{}'; p99 float8; p50 float8;
    hit int := 0; total int := 0; hit_t int := 0; total_t int := 0; extra_t int := 0;
    approx uuid[]; exact uuid[]; approx_t uuid[]; exact_t uuid[];
    -- The pattern from nearest_faces: top k by the index, then the threshold.
    search text := 'WITH nearest AS MATERIALIZED (SELECT user_id, embedding <=> %L::vector(512) AS distance '
                   'FROM face_embeddings ORDER BY embedding <=> %L::vector(512) LIMIT 50) '
                   'SELECT user_id, distance FROM nearest WHERE distance <= 0.32 ORDER BY distance';
    top50 text := 'SELECT array_agg(user_id) FROM (SELECT user_id FROM face_embeddings ORDER BY embedding <=> %L::vector(512) LIMIT 50) s';
BEGIN
    -- Warm-up: one untimed pass, so the timed pass measures a warm index (as the
    -- running API sees it) rather than the first page reads after the seed.
    FOR q IN SELECT v FROM queries LOOP
        PERFORM count(*) FROM nearest_faces(q.v);
    END LOOP;
    FOR q IN SELECT v FROM queries LOOP
        EXECUTE 'EXPLAIN (ANALYZE, TIMING OFF, FORMAT JSON) ' || format(search, q.v, q.v) INTO j;
        IF j::text NOT LIKE '%"Index Name": "face_embeddings_embedding"%' THEN
            RAISE EXCEPTION 'the query did not use the HNSW index: %', j;
        END IF;
        times := times || (j -> 0 ->> 'Execution Time')::float8;
    END LOOP;
    SELECT percentile_disc(0.99) WITHIN GROUP (ORDER BY x), percentile_disc(0.5) WITHIN GROUP (ORDER BY x)
        INTO p99, p50 FROM unnest(times) x;
    IF p99 >= 5 THEN RAISE EXCEPTION 'vector search p99 is % ms (median %), budget is 5 ms', p99, p50; END IF;

    -- Recall against an exact search (index scans off) for the same queries: of the
    -- top 50, and of the matches over the threshold (through nearest_faces itself).
    FOR q IN SELECT v FROM queries LOOP
        EXECUTE format(top50, q.v) INTO approx;
        SELECT array_agg(n.user_id) INTO approx_t FROM nearest_faces(q.v) n;
        PERFORM set_config('enable_indexscan', 'off', true), set_config('enable_seqscan', 'on', true);
        EXECUTE format(top50, q.v) INTO exact;
        SELECT array_agg(user_id) INTO exact_t FROM face_embeddings WHERE embedding <=> q.v <= 0.32;
        PERFORM set_config('enable_indexscan', 'on', true), set_config('enable_seqscan', 'off', true);
        hit := hit + cardinality(ARRAY(SELECT unnest(approx) INTERSECT SELECT unnest(exact)));
        total := total + cardinality(exact);
        hit_t := hit_t + coalesce(cardinality(ARRAY(SELECT unnest(approx_t) INTERSECT SELECT unnest(exact_t))), 0);
        total_t := total_t + coalesce(cardinality(exact_t), 0);
        extra_t := extra_t + coalesce(cardinality(ARRAY(SELECT unnest(approx_t) EXCEPT SELECT unnest(exact_t))), 0);
    END LOOP;
    IF hit::float8 / total < 0.95 THEN RAISE EXCEPTION 'top-50 recall is %, needs 0.95', hit::float8 / total; END IF;
    -- Each noisy query can only match its source, so 1..49 matches means the threshold
    -- both kept and dropped some; nothing past the threshold may come back.
    IF total_t NOT BETWEEN 1 AND 49 THEN RAISE EXCEPTION 'the queries did not straddle the threshold: % matches', total_t; END IF;
    IF extra_t > 0 THEN RAISE EXCEPTION 'nearest_faces returned % rows past the threshold', extra_t; END IF;
    IF hit_t::float8 / total_t < 0.95 THEN RAISE EXCEPTION 'thresholded recall is %, needs 0.95', hit_t::float8 / total_t; END IF;
    RAISE NOTICE 'ok: vector search p99 % ms (median %) over 100 queries, HNSW index used, top-50 recall %, thresholded recall % (% matches)',
        round(p99::numeric, 3), round(p50::numeric, 3), round(hit::numeric / total, 4), round(hit_t::numeric / total_t, 4), total_t;
END
$$;
