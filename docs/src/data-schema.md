# Data, schema, diff, transfer, and admin

- Grid edits accumulate in a local change set and apply only after review.
- Schema forms preview dialect-quoted DDL.
- Schema diff produces a reviewed script with risk markers.
- Import/export streams CSV, TSV, JSON, JSONL, and SQL. Native dump tools never receive passwords on argv.
- Admin lists sessions, locks, and sizes when the driver capability is available.
