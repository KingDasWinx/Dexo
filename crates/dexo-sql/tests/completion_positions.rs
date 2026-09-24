use dexo_sql::{
    CompletionItem, CompletionKind, Dialect, FakeCatalog, Intent, TriggerMode, TriggerOrigin,
    analyze, complete, should_open,
};

fn catalog() -> FakeCatalog {
    let mut catalog = FakeCatalog::default();
    catalog.add_table(
        "public.users",
        ["id", "email", "name", "created_at"],
        false,
        0,
    );
    catalog.add_table(
        "public.orders",
        ["id", "user_id", "total", "status"],
        false,
        0,
    );
    catalog.add_foreign_key("public.orders", ["user_id"], "public.users", ["id"]);
    catalog
}

/// `text` with the cursor written as `|`.
fn split(text: &str) -> (String, usize) {
    let at = text.find('|').expect("no cursor in the sample");
    (text.replacen('|', "", 1), at)
}

fn intent(text: &str) -> Intent {
    let (sql, at) = split(text);
    analyze(&sql, at, Dialect::Postgres).intent
}

fn items(text: &str) -> Vec<CompletionItem> {
    let (sql, at) = split(text);
    complete(&sql, at, &catalog(), Dialect::Postgres)
}

fn labels(text: &str) -> Vec<String> {
    items(text).into_iter().map(|item| item.label).collect()
}

fn columns(text: &str) -> Vec<String> {
    items(text)
        .into_iter()
        .filter(|item| item.kind == CompletionKind::Column)
        .map(|item| item.label)
        .collect()
}

fn opens_while_typing(text: &str) -> bool {
    let (sql, at) = split(text);
    let context = analyze(&sql, at, Dialect::Postgres);
    should_open(TriggerMode::Positional, &context, TriggerOrigin::Typing)
}

/// The old analysis only read the one word before the cursor, so a column after a
/// comma, an operator, `(` or DISTINCT got the table list or nothing.
#[test]
fn every_place_an_expression_goes_offers_the_columns_in_scope() {
    for sample in [
        "select | from users",
        "select id, | from users",
        "select count(|) from users",
        "select distinct | from users",
        "select * from users where |",
        "select * from users where id = 1 and |",
        "select * from users where id = |",
        "select * from users where not |",
        "select * from users where id in (1, |)",
        "select * from users order by |",
        "select * from users order by id desc, |",
        "select case when | from users",
        "select * from users having |",
        "update users set name = | where id = 1",
        "update users set name = 'x' where |",
        "delete from users where |",
    ] {
        assert_eq!(intent(sample), Intent::Column, "{sample}");
        let columns = columns(sample);
        assert!(
            columns.contains(&"email".to_string()),
            "{sample}: {columns:?}"
        );
    }
}

#[test]
fn the_prefix_narrows_columns_and_functions_alike() {
    let found = labels("select * from users where em|");
    assert_eq!(found.first().map(String::as_str), Some("email"));

    let found = labels("select co| from orders");
    assert!(found.contains(&"count".into()), "{found:?}");
    assert!(found.contains(&"coalesce".into()), "{found:?}");
}

#[test]
fn a_subquery_sees_its_own_tables() {
    let found = columns("select * from users where id in (select user_id from orders where |)");
    assert!(found.contains(&"total".into()), "{found:?}");
}

#[test]
fn names_of_the_tables_in_the_statement_are_offered() {
    let found = items("select * from users u join orders o on o.user_id = u.id where |");
    assert!(
        found
            .iter()
            .any(|item| item.kind == CompletionKind::Alias && item.label == "o"),
        "{:?}",
        found.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
}

#[test]
fn after_on_the_foreign_key_comes_first() {
    assert_eq!(
        intent("select * from users u join orders o on |"),
        Intent::JoinCondition
    );
    let found = labels("select * from users u join orders o on |");
    assert_eq!(found.first().map(String::as_str), Some("o.user_id = u.id"));
}

#[test]
fn a_qualifier_narrows_to_that_table() {
    assert_eq!(
        columns("select u.| from users u"),
        ["id", "email", "name", "created_at"]
    );
    let found = columns("select * from users u join orders o on o.user_id = u.id where o.st|");
    assert_eq!(found, ["status"]);
}

#[test]
fn table_positions_offer_tables() {
    for sample in [
        "select * from |",
        "select * from users, |",
        "select * from users u join |",
        "select * from users u left join |",
        "select * from users u inner join orders o on o.user_id = u.id join |",
        "insert into |",
        "update |",
        "delete from |",
        "truncate table |",
    ] {
        assert_eq!(intent(sample), Intent::Table, "{sample}");
        assert!(opens_while_typing(sample), "{sample}");
        let found = labels(sample);
        assert!(found.contains(&"orders".into()), "{sample}: {found:?}");
    }
    assert_eq!(labels("select * from public.ord|"), ["orders"]);
}

/// Offering names where one is being made up turned `from users u` + Enter into
/// `from users users`.
#[test]
fn where_a_name_is_being_declared_nothing_opens_on_its_own() {
    for sample in [
        "select * from users |",
        "select * from users u|",
        "select * from users u join orders |",
        "select id as |",
    ] {
        assert_eq!(intent(sample), Intent::Alias, "{sample}");
        assert!(!opens_while_typing(sample), "{sample}");
    }
    // Asked for, the next clause is on offer.
    let found = labels("select * from users w|");
    assert_eq!(found.first().map(String::as_str), Some("where"));
}

#[test]
fn insert_and_update_offer_the_target_columns() {
    for sample in ["insert into orders (|", "insert into orders (id, |)"] {
        assert_eq!(intent(sample), Intent::InsertColumn, "{sample}");
        assert!(opens_while_typing(sample), "{sample}");
        assert!(columns(sample).contains(&"total".into()), "{sample}");
    }
    for sample in ["update orders set |", "update orders set status = 'x', |"] {
        assert_eq!(intent(sample), Intent::UpdateColumn, "{sample}");
        assert!(columns(sample).contains(&"total".into()), "{sample}");
    }
    assert!(!opens_while_typing("insert into orders (id) values (|"));
}

#[test]
fn values_and_limits_do_not_open_by_themselves() {
    for sample in [
        "insert into orders (id) values (|",
        "select * from users limit |",
        "select * from users where id = 1 |",
        "select count(|",
    ] {
        assert!(!opens_while_typing(sample), "{sample}");
    }
    assert!(opens_while_typing("select * from users where |"));
}

#[test]
fn other_statements_do_not_leak_in() {
    for sample in [
        "select * from users;\nselect * from orders where |",
        "select * from users\nselect * from orders where |",
    ] {
        let found = columns(sample);
        assert!(found.contains(&"total".into()), "{sample}: {found:?}");
        assert!(!found.contains(&"email".into()), "{sample}: {found:?}");
    }
}

#[test]
fn keywords_follow_the_case_being_typed() {
    let found = labels("SEL|");
    assert!(found.contains(&"SELECT".into()), "{found:?}");
    let found = labels("sel|");
    assert!(found.contains(&"select".into()), "{found:?}");
}

#[test]
fn a_cte_and_its_query_are_one_statement() {
    let found =
        columns("with recent as (select * from orders)\nselect * from recent r join users u on |");
    assert!(found.contains(&"email".into()), "{found:?}");
}

#[test]
fn nothing_inside_strings_or_comments() {
    assert_eq!(intent("select 'fr|'"), Intent::Suppressed);
    assert_eq!(intent("select 1 -- fr|"), Intent::Suppressed);
}
