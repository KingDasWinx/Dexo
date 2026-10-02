use duckdb::Connection;
fn q(conn: &Connection, sql: &str) {
    match conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0)) {
        Ok(v) => println!("{sql:70} => {v:?}"),
        Err(e) => println!("{sql:70} => ERR {e}"),
    }
}
fn main() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE t(a INT); CREATE SEQUENCE s;")
        .unwrap();
    for s in [
        "select 1",
        "from t",
        "summarize t",
        "describe t",
        "show tables",
        "pivot t on a using count(*)",
        "select 1; select 2",
        "select 1 --\r; copy t to 'x.csv' --\n",
        "explain select 1",
        "values (1)",
        "with x as (select 1) select * from x",
        "call pragma_version()",
        "pragma version",
        "select nextval('s')",
        "insert into t values (1)",
        "from checkpoint()",
    ] {
        let lit = s.replace('\'', "''");
        let r: Result<String, _> = conn.query_row(
            &format!("SELECT json_serialize_sql('{lit}')::VARCHAR"),
            [],
            |r| r.get(0),
        );
        let text = r
            .map(|t| t.chars().take(110).collect::<String>())
            .unwrap_or_else(|e| format!("ERR {e}"));
        println!("JSON {s:45?} => {text}");
    }
    let r: Result<String, _> = conn.query_row(
        "SELECT json_serialize_sql($1::VARCHAR)::VARCHAR",
        ["select 1"],
        |r| r.get(0),
    );
    println!(
        "param cast => {:?}",
        r.map(|t| t.chars().take(60).collect::<String>())
    );
    for e in [
        "['a', 'a b', 'a,b', 'NULL', NULL, '', ' x', 'it''s', '[x]']::VARCHAR",
        "{'a': 'x y', 'b': NULL, 'c': 1}::VARCHAR",
        "MAP {'k': 'v,1', 'z': NULL}::VARCHAR",
        "MAP {1: 2}::VARCHAR",
        "[[1,2],[3]]::VARCHAR",
        "[{'a': 1}]::VARCHAR",
        "union_value(num := 2)::UNION(num INT, str VARCHAR)::VARCHAR",
        "INTERVAL '1 year 2 months 3 days 04:05:06.5'::VARCHAR",
        "INTERVAL '4.5 seconds'::VARCHAR",
        "INTERVAL '-1 day'::VARCHAR",
        "INTERVAL '36 hours'::VARCHAR",
        "1e300::DOUBLE::VARCHAR",
        "0.1::DOUBLE::VARCHAR",
        "'nan'::DOUBLE::VARCHAR",
        "'-inf'::DOUBLE::VARCHAR",
        "1.0::DOUBLE::VARCHAR",
        "0.1::FLOAT::VARCHAR",
        "123456789.0::DOUBLE::VARCHAR",
        "1e-7::DOUBLE::VARCHAR",
        "1e16::DOUBLE::VARCHAR",
        "'0044-03-15 (BC)'::DATE::VARCHAR",
        "'12345-01-01'::DATE::VARCHAR",
        "'infinity'::DATE::VARCHAR",
        "'-infinity'::TIMESTAMP::VARCHAR",
        "'2020-01-01 10:00:00.5'::TIMESTAMP::VARCHAR",
        "'2020-01-01 10:00:00'::TIMESTAMP::VARCHAR",
        "'2020-01-01 10:00:00.123456789'::TIMESTAMP_NS::VARCHAR",
        "'2020-01-01 10:00:00'::TIMESTAMP_S::VARCHAR",
        "'12:00:00+05:30'::TIMETZ::VARCHAR",
        "'12:00:00.25'::TIME::VARCHAR",
        "'2020-01-01 10:00:00+00'::TIMESTAMPTZ::VARCHAR",
        "current_setting('TimeZone')",
        "340282366920938463463374607431768211455::UHUGEINT::VARCHAR",
        "'123456789012345678901234567890'::BIGNUM::VARCHAR",
        "[1,2]::INTEGER[2]::VARCHAR",
        "'0101'::BIT::VARCHAR",
        "'\\xAA\\x00'::BLOB::VARCHAR",
        "['2020-01-01'::DATE]::VARCHAR",
        "[1.5::DOUBLE]::VARCHAR",
        "[true]::VARCHAR",
        "{'a': [1,2]}::VARCHAR",
        "typeof(union_value(num := 2)::UNION(num INT, str VARCHAR))",
    ] {
        q(&conn, &format!("SELECT {e}"));
    }
    // lossless arrow
    conn.execute_batch("SET arrow_lossless_conversion = true")
        .unwrap();
    let mut stmt = conn.prepare("SELECT '12:00:00+05:30'::TIMETZ a, 340282366920938463463374607431768211455::UHUGEINT b, '123'::BIGNUM c, uuid() d, 1::HUGEINT e, union_value(num := 2)::UNION(num INT, str VARCHAR) f, INTERVAL 1 DAY g").unwrap();
    let _ = stmt.stream_arrow([]).unwrap();
    for f in stmt.schema().fields() {
        println!(
            "lossless {} {:?} {:?}",
            f.name(),
            f.data_type(),
            f.metadata()
        );
    }
    let _ = conn.execute_batch("SET arrow_lossless_conversion = false");
    let mut stmt = conn.prepare("SELECT '12:00:00+05:30'::TIMETZ a, union_value(num := 2)::UNION(num INT, str VARCHAR) f, NULL::UNION(num INT, str VARCHAR) g, 'infinity'::DATE h").unwrap();
    let _ = stmt.stream_arrow([]).unwrap();
    for f in stmt.schema().fields() {
        println!("default {} {:?}", f.name(), f.data_type());
    }
    println!("settings: {:?}", conn.query_row("SELECT current_setting('autoinstall_known_extensions')::VARCHAR || ' ' || current_setting('autoload_known_extensions')::VARCHAR", [], |r| r.get::<_, String>(0)));
    let p = std::env::temp_dir().join("dk-probe.duckdb");
    let _ = std::fs::remove_file(&p);
    let a = Connection::open(&p).unwrap();
    a.execute_batch("create table x(i int)").unwrap();
    let b = a.try_clone().unwrap();
    b.execute_batch("insert into x values (1)").unwrap();
    println!(
        "clone sees: {:?}",
        a.query_row("select count(*) from x", [], |r| r.get::<_, i64>(0))
    );
    drop(a);
    drop(b);
    let _ = std::fs::remove_file(&p);
}
