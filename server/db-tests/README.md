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
dev database alone. Each check is commented in `run.sh` and `acceptance.sql`. Timings are
database execution time from `EXPLAIN ANALYZE`.
