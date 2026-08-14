use dexo_runtime::TaskRegistry;

#[tokio::test]
async fn cancellation_reaches_registered_task() {
    let registry = TaskRegistry::default();
    let task = registry.register();
    assert!(!task.token.is_cancelled());
    assert!(registry.cancel(task.id));
    task.token.cancelled().await;
    assert!(task.token.is_cancelled());
}
