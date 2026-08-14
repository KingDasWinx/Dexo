use dexo_sql::split_statements;

#[test]
fn fuzz_smoke_statement_splitter() {
    for bytes in [
        b"" as &[u8],
        b"select 1",
        b"select 1; select 2;",
        b"/* ; */ select 1",
        b"'\x00\xff",
        b"CREATE TABLE t (id int);",
    ] {
        let _ = split_statements(&String::from_utf8_lossy(bytes));
    }
}
