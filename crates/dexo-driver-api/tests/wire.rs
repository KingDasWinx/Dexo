//! The types a driver plugin sends Dexo travel as JSON: every one comes back as it left.
use dexo_driver_api::{
    Capability, CapabilityState, CatalogListOptions, ColumnId, ColumnMeta, ConnectionSecrets,
    DataRequest, DbValue, DriverError, DriverErrorCategory, ExplainRequest, Filter, Mutation, Page,
    QualifiedName, QueryEvent, RawClauses, RowBatch, Sort, TransactionMode, TransactionState,
};
use serde_json::json;

fn round_trip<T>(value: T) -> serde_json::Value
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_value(&value).unwrap();
    let back: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, value);
    json
}

#[test]
fn values_are_tagged_and_bytes_are_base64() {
    assert_eq!(round_trip(DbValue::I64(1)), json!({"t": "i64", "v": 1}));
    assert_eq!(round_trip(DbValue::Null), json!({"t": "null"}));
    assert_eq!(
        round_trip(DbValue::Bytes(vec![0, 255, 10])),
        json!({"t": "bytes", "v": "AP8K"})
    );
    round_trip(DbValue::U64(u64::MAX));
    round_trip(DbValue::Decimal("12345678901234567890.123".into()));
    round_trip(DbValue::Json(r#"{"a":[1,2]}"#.into()));
    round_trip(DbValue::Native {
        type_name: "interval".into(),
        bytes: vec![1, 2],
        text: "1 day".into(),
    });
}

#[test]
fn events_and_requests_round_trip() {
    round_trip(QueryEvent::Columns(vec![ColumnMeta {
        name: "id".into(),
        type_name: "int8".into(),
        nullable: false,
    }]));
    assert_eq!(
        round_trip(QueryEvent::Rows(RowBatch {
            rows: vec![vec![DbValue::I64(1)]]
        })),
        json!({"rows": [[{"t": "i64", "v": 1}]]})
    );
    round_trip(QueryEvent::ResultSetStarted { index: 0 });
    round_trip(QueryEvent::ResultSetFinished {
        index: 0,
        rows_affected: Some(3),
        truncated: true,
    });
    round_trip(QueryEvent::Notice {
        message: "hi".into(),
    });
    round_trip(QueryEvent::Finished {
        rows_affected: None,
    });
    round_trip(CapabilityState::unavailable(Capability::Ddl, "no DDL"));
    round_trip(CapabilityState::available(Capability::Query));
    round_trip(TransactionState::Failed);
    round_trip(TransactionMode::ReadOnly);
    round_trip(CatalogListOptions {
        include_system: true,
    });
    round_trip(ExplainRequest::with_indexes(
        "select 1",
        vec!["create index on t (a)".into()],
    ));
    round_trip(Mutation::Update {
        table: QualifiedName::new(None::<String>, Some("main"), "t"),
        identity: vec![(ColumnId("id".into()), DbValue::I64(1))],
        original: vec![],
        changes: vec![(ColumnId("name".into()), DbValue::Text("x".into()))],
    });
    round_trip(DataRequest {
        object: QualifiedName::new(None::<String>, None::<String>, "t"),
        columns: vec![],
        filter: Some(Filter::Not(Box::new(Filter::IsNull(ColumnId("a".into()))))),
        sort: vec![Sort {
            column: ColumnId("a".into()),
            descending: true,
        }],
        page: Page::new(0, 50).unwrap(),
        clauses: RawClauses::default(),
    });
}

#[test]
fn a_driver_error_keeps_every_field() {
    let error = DriverError::new(
        DriverErrorCategory::Syntax,
        "syntax error at or near \"selec\"",
    )
    .with_native_code("42601")
    .with_position(1)
    .with_row(2)
    .with_detail("d")
    .with_hint("h")
    .retryable();
    let back: DriverError = serde_json::from_value(serde_json::to_value(&error).unwrap()).unwrap();
    assert_eq!(back.to_string(), error.to_string());
    assert_eq!(back.category(), DriverErrorCategory::Syntax);
    assert_eq!(back.native_code(), Some("42601"));
    assert_eq!((back.position(), back.row()), (Some(1), Some(2)));
    assert_eq!(
        (back.detail(), back.hint(), back.is_retryable()),
        (Some("d"), Some("h"), true)
    );
}

#[test]
fn secrets_list_their_purposes() {
    let mut secrets = ConnectionSecrets::database_password("pw".to_string().into());
    secrets.insert("ssh_passphrase", "pp".to_string().into());
    let purposes: Vec<&str> = secrets.iter().map(|(purpose, _)| purpose).collect();
    assert_eq!(purposes, ["database_password", "ssh_passphrase"]);
}
