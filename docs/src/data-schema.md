# Data, schema, diff, transfer, and admin

- Rows inserted or deleted in the grid, and cells changed there (F2 on a cell of a table's rows: type the value, set NULL, or open it in `$EDITOR`), accumulate in a local change set and apply only after review (Ctrl+S): the review shows each statement with its values, and Apply runs them. A read-only connection, a view, a table with no primary key or unique column, and the column a row is found by refuse a change before it is staged, and say why.
- Schema forms preview dialect-quoted DDL from the selected driver and require typed confirmation for destructive changes.
- Schema diff compares live, saved, and imported snapshots, then applies only a freshly reviewed plan.
- Import streams CSV, TSV, JSON and JSONL into a table; export writes those and SQL `INSERT`s into the table the file is named after (`orders.sql` inserts into `orders`; `dexo export --table` names another). An SQL file is a script: run it in the editor or with `dexo run --file`. Native dump tools never receive passwords on argv.
- Sessions (in the palette) lists the server's sessions and who blocks whom on Postgres, MySQL and MariaDB: the arrows pick one, `t` ends it once its id is typed, `r` reloads the list. A read-only connection ends none. `dexo sessions list|cancel|terminate` does the same from the command line.
- EXPLAIN uses the statement under the cursor. EXPLAIN ANALYZE requires a dedicated confirmation.
