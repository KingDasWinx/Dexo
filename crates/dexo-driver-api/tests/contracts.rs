use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DbValue, DriverError,
    QualifiedName, Session,
};

#[test]
fn value_and_identifier_preserve_native_information() {
    let name = QualifiedName::new(Some("db"), Some("public"), "orders");
    assert_eq!(name.display_unquoted(), "db.public.orders");
    let value = DbValue::Native {
        type_name: "ltree".into(),
        bytes: b"a.b".to_vec(),
        text: "a.b".into(),
    };
    assert_eq!(value.type_name(), Some("ltree"));
}

#[test]
fn unavailable_capability_keeps_reason() {
    let state = CapabilityState::unavailable(Capability::ExplainAnalyze, "server version");
    assert_eq!(state.reason(), Some("server version"));
}

struct FakeFactory;

#[async_trait::async_trait]
impl ConnectionFactory for FakeFactory {
    fn driver_name(&self) -> &'static str {
        "fake"
    }

    async fn connect(&self, _: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        Err(DriverError::unsupported("fake"))
    }
}

#[test]
fn factory_is_object_safe() {
    let _: Box<dyn ConnectionFactory> = Box::new(FakeFactory);
}
