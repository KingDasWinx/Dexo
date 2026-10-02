# Data, schema, diff, transfer, and admin

- Grid edits accumulate in a local change set and apply only after review.
- Schema forms preview dialect-quoted DDL from the selected driver and require typed confirmation for destructive changes.
- Schema diff compares live, saved, and imported snapshots, then applies only a freshly reviewed plan.
- Import streams CSV, TSV, JSON and JSONL into a table; export writes those and SQL `INSERT`s into the table the file is named after (`orders.sql` inserts into `orders`; `dexo export --table` names another). An SQL file is a script: run it in the editor or with `dexo run --file`. Native dump tools never receive passwords on argv.
- Admin lists sessions, locks, and sizes when the driver capability is available.
- EXPLAIN uses the statement under the cursor. EXPLAIN ANALYZE requires a dedicated confirmation.
