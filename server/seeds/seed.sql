-- Synthetic dev and test data. Not a migration: run it once on a freshly migrated,
-- empty database:
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f server/seeds/seed.sql
-- Fixed random seed, so every run makes the same data.
BEGIN;
SELECT setseed(0.42) \gset

-- 1,000 hackers; three in four are checked in.
INSERT INTO users (email, full_name, school, qr_hmac_sig, checked_in_at)
SELECT format('hacker%s@example.test', i), format('Hacker %s', i), 'Synthetic University',
       sha256(convert_to('seed-' || i, 'UTF8')),
       CASE WHEN i % 4 <> 0 THEN timestamptz '2027-02-20 09:00-06' END
FROM generate_series(1, 1000) i;

-- 250 teams of 4.
INSERT INTO teams (name, opted_tracks)
SELECT format('Team %s', i), CASE WHEN i % 2 = 0 THEN '{GENERAL,acme}'::text[] ELSE '{GENERAL}' END
FROM generate_series(1, 250) i;
INSERT INTO team_members (user_id, team_id)
SELECT u.id, t.id
FROM (SELECT id, (row_number() OVER (ORDER BY id) - 1) / 4 AS n FROM users) u
JOIN (SELECT id, row_number() OVER (ORDER BY id) - 1 AS n FROM teams) t USING (n);

-- 25 presentation rooms, judging on Feb 21 2027, 10 teams in each room's queue.
INSERT INTO presentation_rooms (room_id, track, floor_number, svg_x, svg_y, judging_starts_at, judging_ends_at)
SELECT format('ROOM-%s', lpad(i::text, 2, '0')), 'GENERAL', 1 + i % 3, random() * 1000, random() * 600,
       timestamptz '2027-02-21 09:00-06', timestamptz '2027-02-21 13:00-06'
FROM generate_series(1, 25) i;
INSERT INTO judging_queue (room_id, team_id, track, seq_index)
SELECT format('ROOM-%s', lpad((1 + n % 25)::text, 2, '0')), id, 'GENERAL', n / 25
FROM (SELECT id, row_number() OVER (ORDER BY id) - 1 AS n FROM teams) t;

-- 5 entitlements, each scanned once for every hacker: 5,000 scans.
INSERT INTO entitlements (entitlement_id, label) VALUES
    ('checkin', 'Check-in'), ('shirt', 'T-shirt'), ('dinner-1', 'Dinner, day 1'),
    ('breakfast-2', 'Breakfast, day 2'), ('lunch-2', 'Lunch, day 2');
INSERT INTO entitlement_scans (user_id, entitlement_id, client_scanned_at)
SELECT u.id, e.entitlement_id, timestamptz '2027-02-20 09:00-06' + random() * interval '30 hours'
FROM users u CROSS JOIN entitlements e;

-- 500 hardware assets, ASSET-001 to ASSET-500. No loans.
INSERT INTO hardware_assets (asset_qr, category)
SELECT format('ASSET-%s', lpad(i::text, 3, '0')), (ARRAY['arduino', 'raspberry-pi', 'sensor-kit', 'vr-headset'])[1 + i % 4]
FROM generate_series(1, 500) i;

-- 300 unclaimed keys for each of two sponsors.
INSERT INTO sponsor_keys (sponsor_slug, key_value)
SELECT s, format('%s-%s', s, md5(random()::text))
FROM unnest(ARRAY['acme', 'globex']) s, generate_series(1, 300);

-- 1,000 random unit-length 512-dimension face embeddings, one per hacker.
INSERT INTO face_embeddings (user_id, embedding)
SELECT u.id, l2_normalize(ARRAY(SELECT random_normal() FROM generate_series(1, 512) WHERE u.id IS NOT NULL)::vector(512))
FROM users u;

COMMIT;
-- Sets hint bits and the visibility map now, so autovacuum doesn't run during the tests.
VACUUM ANALYZE;
