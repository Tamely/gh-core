# Schema checks

`run.sh` checks `server/migrations` against the schema ticket. It needs Docker and
[sqlx-cli](https://crates.io/crates/sqlx-cli) (`brew install sqlx-cli`, or
`cargo install sqlx-cli --no-default-features --features postgres`). CI runs the same script
(`.github/workflows/schema.yml`).

```
server/db-tests/run.sh            # every check
server/db-tests/run.sh --update   # after changing a migration: rewrite server/schema.sql, commit it
```

It uses its own database, `ghcore_schema_test`, in the compose Postgres, and leaves your
dev database alone. What it checks:

1. Every migration up, then a rerun does nothing; all down leaves no gh-core objects;
   up again gives the same `pg_dump --schema-only`, equal to the committed `server/schema.sql`.
2. Seeds the database with `server/seeds/seed.sql`.
3. `acceptance.sql`: every foreign key has an explicit ON DELETE and an index; the
   `updated_at` trigger; a duplicate scan gives 23505, or 0 rows with ON CONFLICT DO NOTHING,
   p99 under 1ms over 1,000 attempts on 5,000 rows; an unknown entitlement fails the foreign key;
   one open loan per asset; deleting a user deletes their face embedding; vector search
   p99 under 5ms over 100 queries using the HNSW index, recall at least 0.95 against an
   exact scan.
4. A `judging_queue` swap commits, and a leftover duplicate fails at COMMIT.
5. 50 concurrent sponsor key claims (`claim.pgbench`) get 50 different keys, none waits.

Timings are database execution time from `EXPLAIN ANALYZE`.
