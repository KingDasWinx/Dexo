//! `:name` placeholders are read off the lexer's tokens. A raw byte scan took every
//! colon followed by a letter, so a schema script whose comments said `N:N` opened the
//! parameter prompt asking for `N`, and every Postgres cast asked for its type.
use dexo_sql::{Dialect, named_parameters};

fn names(sql: &str) -> Vec<String> {
    named_parameters(sql, Dialect::Postgres)
        .into_iter()
        .map(|parameter| parameter.name)
        .collect()
}

/// The script that reported it: a schema with no placeholders, whose comments and one
/// `COMMENT ON` string say `N:N`.
#[test]
fn a_schema_script_with_no_placeholders_asks_for_nothing() {
    let sql = include_str!("fixtures_schema_vendas.sql");
    assert_eq!(names(sql), Vec::<String>::new());
}

#[test]
fn colons_outside_code_are_not_placeholders() {
    for sql in [
        "-- tabela associativa N:N\nselect 1",
        "select 'Associativa N:N entre'",
        "/* ratio a:b */ select 1",
        "select \"weird:col\" from t",
        "create function f() returns int as $$ select 1 where x = :y $$ language sql",
    ] {
        assert_eq!(names(sql), Vec::<String>::new(), "in {sql:?}");
    }
}

/// The widest of them: every cast in a statement used to ask for a value.
#[test]
fn a_postgres_cast_is_not_a_placeholder() {
    assert_eq!(
        names("select id::text, created_at::date from t"),
        Vec::<String>::new()
    );
    assert_eq!(
        names("select :id::int"),
        vec!["id".to_string()],
        "a cast right after a real placeholder"
    );
}

#[test]
fn real_placeholders_are_still_found_once_each() {
    assert_eq!(
        names("select * from t where id = :id"),
        vec!["id".to_string()]
    );
    assert_eq!(
        names("select * from t where a = :a and b = :b or a = :a"),
        vec!["a".to_string(), "b".to_string()]
    );
}
