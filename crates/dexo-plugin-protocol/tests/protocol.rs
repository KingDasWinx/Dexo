use std::io::BufReader;

use dexo_driver_api::{ConnectRequest, DriverError, DriverErrorCategory, QueryRequest};
use dexo_plugin_protocol::{
    Family, QueryExecute, Request, RpcError, SessionOpenParams, SessionOpenResult, code, read_line,
    read_line_with, write_line,
};
use secrecy::{ExposeSecret, SecretString};

#[test]
fn messages_are_one_per_line() {
    let mut wire = Vec::new();
    write_line(&mut wire, &Request::new(1, "initialize", ()).unwrap()).unwrap();
    write_line(&mut wire, &Request::new(2, "session/close", ()).unwrap()).unwrap();
    wire.extend_from_slice(b"{\"last\":true}");
    let mut reader = BufReader::new(&wire[..]);
    let mut line = Vec::new();
    assert!(read_line(&mut reader, &mut line).unwrap());
    let first: Request = serde_json::from_slice(&line).unwrap();
    assert_eq!((first.id, first.method.as_str()), (1, "initialize"));
    assert!(read_line(&mut reader, &mut line).unwrap());
    assert!(read_line(&mut reader, &mut line).unwrap());
    assert_eq!(line, b"{\"last\":true}");
    assert!(!read_line(&mut reader, &mut line).unwrap());
}

#[test]
fn a_line_past_the_cap_is_refused() {
    let mut reader = BufReader::new(&b"0123456789\n"[..]);
    let mut line = Vec::new();
    let error = read_line_with(&mut reader, &mut line, 8).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn opening_a_session_carries_the_secrets_and_never_prints_them() {
    let mut request = ConnectRequest::new(
        "db.example:5432",
        Some("shop".into()),
        "ada".into(),
        SecretString::from("hunter2".to_string()),
        true,
    );
    request.secrets.insert(
        "ssh_passphrase",
        SecretString::from("s3cr3t-pp".to_string()),
    );
    let params = SessionOpenParams::new("postgres", &request);
    let debug = format!("{params:?}");
    assert!(
        !debug.contains("hunter2") && !debug.contains("s3cr3t"),
        "{debug}"
    );
    let json = serde_json::to_string(&params).unwrap();
    let back: SessionOpenParams = serde_json::from_str(&json).unwrap();
    assert_eq!(back.driver, "postgres");
    let restored = back.into_request();
    assert_eq!(restored.secret.expose_secret(), "hunter2");
    assert_eq!(
        restored
            .secrets
            .get("ssh_passphrase")
            .unwrap()
            .expose_secret(),
        "s3cr3t-pp"
    );
    assert_eq!(restored.transport, request.transport);
    assert_eq!(restored.database.as_deref(), Some("shop"));
    assert!(restored.read_only);
}

#[test]
fn a_driver_error_crosses_the_wire_whole() {
    let error = DriverError::new(DriverErrorCategory::Syntax, "near \"selec\": syntax error")
        .with_position(1);
    let back = RpcError::driver(&error).into_driver_error();
    assert_eq!(back.category(), DriverErrorCategory::Syntax);
    assert_eq!(back.position(), Some(1));
    let other = RpcError::new(code::INTERNAL, "boom").into_driver_error();
    assert_eq!(other.category(), DriverErrorCategory::Internal);
    assert_eq!(other.to_string(), "boom");
}

#[test]
fn what_a_newer_plugin_adds_is_skipped() {
    let json = r#"{"session":"s1",
        "capabilities":[{"capability":"query","available":true},{"capability":"teleport","available":true}],
        "families":["catalog","teleport"]}"#;
    let opened: SessionOpenResult = serde_json::from_str(json).unwrap();
    assert_eq!(opened.families, [Family::Catalog]);
    assert_eq!(opened.capabilities.len(), 1);
    assert_eq!(opened.transaction, None);
}

#[test]
fn a_query_request_keeps_its_timeout_and_flags() {
    let mut request = QueryRequest::read("select 1", 1000);
    request.timeout = std::time::Duration::from_millis(1500);
    request.read_only = true;
    let back = QueryExecute::new(&request).into_request();
    assert_eq!(back.id, request.id);
    assert_eq!(back.timeout, request.timeout);
    assert_eq!(
        (back.row_limit, back.read_only, back.mutating),
        (1000, true, false)
    );
}
