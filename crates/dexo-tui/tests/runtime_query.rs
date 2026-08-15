use dexo_tui::runtime::{OperationId, OperationKey};

#[test]
fn operation_key_rejects_a_stale_session_generation() {
    let operation = OperationKey::new(OperationId::new(), "session-a", "doc-a", 4);
    assert!(operation.belongs_to("session-a", "doc-a", 4));
    assert!(!operation.belongs_to("session-a", "doc-a", 3));
    assert!(!operation.belongs_to("session-b", "doc-a", 4));
}
