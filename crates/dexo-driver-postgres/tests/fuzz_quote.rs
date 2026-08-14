use dexo_driver_postgres::PgDialect;

#[test]
fn fuzz_smoke_identifier_quoting() {
    for name in ["id", "select", "a\"b", "", "user", "😀"] {
        if name.is_empty() {
            continue;
        }
        let quoted = PgDialect::quote_ident(name);
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
        assert!(!quoted.contains(';'));
    }
}
