#!/usr/bin/env bash
# Schema checks for server/migrations: the schema.sql snapshot and the ticket's
# acceptance criteria. Needs Docker (the compose Postgres) and sqlx-cli on PATH.
#   server/db-tests/run.sh            run every check
#   server/db-tests/run.sh --update   also rewrite server/schema.sql from the migrations
# Uses its own database (ghcore_schema_test), so your dev data is left alone.
set -euo pipefail
cd "$(dirname "$0")/.."
DB=ghcore_schema_test
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/$DB"
compose=(docker compose -f docker-compose.yml)
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

pg() { "${compose[@]}" exec -T postgres "$@"; }
sql() { pg psql -U postgres -d "$DB" -X -q -v ON_ERROR_STOP=1 "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
# Comments, blank-line runs and pg_dump's per-run random \restrict key are dropped so
# dumps from different runs and Postgres 16 minor versions compare equal.
dump() {
    pg pg_dump -U postgres -d "$DB" --schema-only --no-owner --no-privileges -T _sqlx_migrations \
        | grep -vE '^(--|\\(un)?restrict )' | cat -s
}
# Prints sqlx's output only if it fails (sqlx writes its errors to stdout).
migrate() { local out; out=$(sqlx migrate "$@" --source migrations 2>&1) || fail "sqlx migrate $*: $out"; }

"${compose[@]}" up -d --wait postgres >/dev/null
pg dropdb -U postgres --if-exists --force "$DB" 2>/dev/null
pg createdb -U postgres "$DB"

echo "== migrations"
migrate run
dump > "$tmp/first.sql"
out=$(sqlx migrate run --source migrations 2>&1) || fail "rerun failed: $out"
[ -z "$out" ] || fail "rerunning the migrations did something: $out"
dump | diff -u "$tmp/first.sql" - || fail "rerunning the migrations changed the schema"
echo "ok: up, then rerun is a no-op"

migrate revert --target-version 0
left=$(sql -tA <<'SQL'
SELECT string_agg(x, ', ') FROM (
    SELECT 'relation ' || relname FROM pg_class
        WHERE relnamespace = 'public'::regnamespace AND relname NOT LIKE '\_sqlx\_migrations%'
    UNION ALL SELECT 'function ' || proname FROM pg_proc WHERE pronamespace = 'public'::regnamespace
    UNION ALL SELECT 'type ' || typname FROM pg_type
        WHERE typnamespace = 'public'::regnamespace AND typname NOT LIKE '%\_sqlx\_migrations'
    UNION ALL SELECT 'extension ' || extname FROM pg_extension WHERE extname <> 'plpgsql'
) o(x)
SQL
)
[ -z "$left" ] || fail "all down left objects behind: $left"
echo "ok: all down leaves no gh-core objects"

migrate run
dump > "$tmp/second.sql"
diff -u "$tmp/first.sql" "$tmp/second.sql" || fail "up, down, up gave a different schema"
if [ "${1:-}" = --update ]; then cp "$tmp/first.sql" schema.sql; echo "updated schema.sql"; fi
diff -u schema.sql "$tmp/first.sql" \
    || fail "server/schema.sql is out of date: run server/db-tests/run.sh --update and commit it"
echo "ok: up, down, up matches server/schema.sql"

echo "== seed"
sql < seeds/seed.sql >/dev/null
echo "ok: seeded"

echo "== acceptance"
out=$(sql < db-tests/acceptance.sql 2>&1) || { echo "$out"; fail "acceptance.sql"; }
sed -n 's/^NOTICE:  //p' <<< "$out"

# judging_queue: a swap in one transaction commits; a leftover duplicate fails at COMMIT.
ids=$(sql -tA -F ' ' -c "SELECT a.id, b.id FROM judging_queue a JOIN judging_queue b
    ON a.room_id = b.room_id AND a.seq_index = 0 AND b.seq_index = 1 LIMIT 1")
read -r a b <<< "$ids"
sql <<SQL
BEGIN;
UPDATE judging_queue SET seq_index = 1 WHERE id = '$a';
UPDATE judging_queue SET seq_index = 0 WHERE id = '$b';
COMMIT;
SQL
[ "$(sql -tA -c "SELECT seq_index FROM judging_queue WHERE id IN ('$a', '$b') ORDER BY id = '$a'" | tr '\n' ' ')" = "0 1 " ] \
    || fail "swap did not stick"
out=$(sql -v ON_ERROR_STOP=0 -v VERBOSITY=verbose 2>&1 <<SQL
\set VERBOSITY verbose
BEGIN;
UPDATE judging_queue SET seq_index = 0 WHERE id = '$a';
\echo updated-without-error
COMMIT;
SQL
) || true
grep -q updated-without-error <<< "$out" || fail "the duplicate was rejected before COMMIT: $out"
grep -q 'ERROR:  23505' <<< "$out" || fail "a duplicate (room_id, seq_index) committed: $out"
[ "$(sql -tA -c "SELECT seq_index FROM judging_queue WHERE id = '$a'")" = 1 ] || fail "the failed transaction changed a row"
echo "ok: seq_index swap commits; a duplicate fails at COMMIT with 23505"

# 50 concurrent sponsor key claims.
start=$(date +%s)
pg pgbench -U postgres -n -c 50 -j 50 -t 1 -f /dev/stdin "$DB" < db-tests/claim.pgbench > "$tmp/pgbench.log" 2>&1 \
    || { cat "$tmp/pgbench.log"; fail "a sponsor key claim failed or blocked"; }
secs=$(( $(date +%s) - start ))
grep -q 'number of failed transactions: 0' "$tmp/pgbench.log" || { cat "$tmp/pgbench.log"; fail "claims failed"; }
read -r n keys teams <<< "$(sql -tA -F ' ' -c "SELECT count(*), count(DISTINCT key_value), count(DISTINCT claimed_by_team)
    FROM sponsor_keys WHERE claimed_by_team IS NOT NULL")"
[ "$n $keys $teams" = "50 50 50" ] || fail "claims: $n rows, $keys keys, $teams teams (want 50 each)"
# Run one after another, the 0.5s holds would take 25s.
[ "$secs" -lt 10 ] || fail "50 claims took ${secs}s, so they waited on each other"
echo "ok: 50 concurrent claims got 50 different keys in ${secs}s, none blocked (lock_timeout 100ms)"

echo "all schema checks passed"
