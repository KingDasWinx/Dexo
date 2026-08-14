use dexo_app::diagnostic_service::redact_text;
use dexo_app::mcp::profile::McpProfile;
use dexo_app::mcp::service::advertised_tools;
use dexo_app::mcp::{Effect, ObjectPolicy, SelectorRule};
use dexo_app::transfer::native_tool::{NativeToolKind, prepare};
use dexo_mcp::hidden_error;

#[test]
fn sql_scope_deny_does_not_enumerate() {
    let policy = ObjectPolicy::new(vec![
        SelectorRule::parse(Effect::Allow, "db.public.items").unwrap(),
    ]);
    let denied = dexo_app::mcp::selector::ObjectRef::parse("db.secret.passwords");
    assert_eq!(policy.decide(&denied), dexo_app::mcp::Decision::DenyHidden);
    assert_eq!(hidden_error(), "not found");
}

#[test]
fn mcp_does_not_advertise_write_tools_without_grant() {
    let profile = McpProfile::new("assistant");
    let tools = advertised_tools(&profile);
    assert!(!tools.contains(&"data_insert"));
    assert!(!tools.contains(&"schema_apply_ddl"));
}

#[test]
fn redaction_and_stdout_injection_sentinels() {
    let text = "password=SUPER_SECRET_SENTINEL\n{\"jsonrpc\":\"2.0\"}";
    let redacted = redact_text(text);
    assert!(!redacted.contains("SUPER_SECRET_SENTINEL"));
}

#[test]
fn native_tool_args_never_include_secret() {
    let dir = tempfile::tempdir().unwrap();
    let prepared = prepare(
        NativeToolKind::PgDump,
        "SUPER_SECRET_SENTINEL",
        "16.9",
        16,
        dir.path(),
    )
    .unwrap();
    assert!(
        !prepared
            .args
            .iter()
            .any(|arg| arg.contains("SUPER_SECRET_SENTINEL"))
    );
    assert!(!prepared.command_line.contains("SUPER_SECRET_SENTINEL"));
}
