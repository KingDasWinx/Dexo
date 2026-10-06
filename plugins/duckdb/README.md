# DuckDB for Dexo

Opens a DuckDB file, `:memory:`, or a CSV, Parquet or JSON file read as a table.

- From the Plugins screen (`Ctrl+G p`), or `dexo plugin install duckdb`.
- Then add a connection with the DuckDB driver and a path, or run `dexo duckdb:///path/to/file.parquet`.

The engine runs in its own process, started on the first connection and stopped a minute after the last one closes.
