//! What `dexo <url>` says when it cannot read the argument: what is wrong, and what to type.
use dexo_app::connection_url::parse;

#[test]
fn a_file_path_is_pointed_at_the_sqlite_url() {
    let error = parse("/path/shop.sqlite3").err().unwrap().to_string();
    assert!(error.contains("sqlite:///path/to/file"), "{error}");
}

#[test]
fn a_port_out_of_range_is_not_called_a_non_number() {
    let error = parse("postgres://dexo@127.0.0.1:99999/db")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("0 to 65535"), "{error}");
}
