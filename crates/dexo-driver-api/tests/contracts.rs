use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DbValue, DriverError,
    QualifiedName, Session,
};
use dexo_driver_postgres::PostgresFactory;

#[test]
fn factories_describe_connection_defaults_without_tui_hardcoding() {
    let descriptor = PostgresFactory.descriptor();
    assert_eq!(descriptor.id, "postgres");
    assert_eq!(descriptor.default_port, 5432);
    assert!(descriptor.options.tls && descriptor.options.ssh && descriptor.options.proxy);
}

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
    fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
        dexo_driver_api::DriverDescriptor {
            id: "fake",
            display_name: "Fake",
            default_port: 1,
            options: dexo_driver_api::ConnectionOptions {
                tls: false,
                client_certificate: false,
                ssh: false,
                proxy: false,
            },
        }
    }

    async fn connect(&self, _: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        Err(DriverError::unsupported("fake"))
    }
}

#[test]
fn factory_is_object_safe() {
    let _: Box<dyn ConnectionFactory> = Box::new(FakeFactory);
}

#[test]
fn explain_request_keeps_analyze_explicit() {
    use dexo_driver_api::ExplainRequest;
    assert!(!ExplainRequest::estimated("select 1").analyze);
    assert!(ExplainRequest::analyzed("select 1").analyze);
}

#[test]
fn missing_admin_metrics_are_none_not_zero() {
    use dexo_driver_api::SizeInfo;
    let size = SizeInfo {
        object: "public.items".into(),
        native_size: None,
        bytes: None,
    };
    assert!(size.bytes.is_none());
    assert!(size.native_size.is_none());
}
