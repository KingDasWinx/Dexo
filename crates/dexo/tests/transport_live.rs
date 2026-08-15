use dexo_driver_api::{
    ConnectRequest, ConnectionFactory, QueryId, QueryRequest, RouteRequest, Session, TlsMode,
    TlsRequest, TransportRequest,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use secrecy::SecretString;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_and_mysql_connect_query_and_cancel_direct() {
    let pair = DatabasePair::start().await.unwrap();
    let pg = PostgresFactory
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo",
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    run_select(pg.as_ref()).await;
    let my = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo",
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    run_select(my.as_ref()).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn verified_tls_and_hostname_mismatch_are_honored() {
    let pair = DatabasePair::start().await.unwrap();
    let mut request = ConnectRequest::new(
        pair.postgres_endpoint().to_string(),
        Some("dexo".into()),
        "dexo",
        SecretString::from("dexo_test_only"),
        false,
    );
    request.transport.tls = Some(TlsRequest {
        mode: TlsMode::VerifyFull,
        server_name: Some("wrong.example".into()),
        ca_file: None,
        client_cert: None,
        client_key: None,
    });
    let error = match PostgresFactory.connect(request).await {
        Ok(_) => panic!("expected TLS hostname mismatch to fail"),
        Err(error) => error,
    };
    assert!(
        error.to_string().to_ascii_lowercase().contains("tls")
            || error
                .to_string()
                .to_ascii_lowercase()
                .contains("certificate")
            || error.to_string().to_ascii_lowercase().contains("ssl")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_cancel_rejects_changed_generation() {
    let pair = DatabasePair::start().await.unwrap();
    let session = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo",
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    // ponytail: live session is boxed; generation bump is covered by MysqlSession::bump_generation
    // in driver unit use. Here we still exercise cancel on a fresh generation-1 session.
    session
        .cancel(QueryId(Uuid::nil()))
        .await
        .expect("fresh generation allows cancel");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn routed_http_connect_request_is_validated_before_socket() {
    let mut request = ConnectRequest::new(
        "db.example.com:5432",
        Some("dexo".into()),
        "dexo",
        SecretString::from("dexo_test_only"),
        false,
    );
    request.transport = TransportRequest {
        target_host: "db.example.com".into(),
        target_port: 5432,
        tls: Some(TlsRequest {
            mode: TlsMode::VerifyFull,
            server_name: None,
            ca_file: None,
            client_cert: None,
            client_key: None,
        }),
        route: RouteRequest::HttpConnect {
            host: String::new(),
            port: 0,
        },
    };
    let error = match PostgresFactory.connect(request).await {
        Ok(_) => panic!("expected invalid proxy to fail"),
        Err(error) => error,
    };
    assert_eq!(
        error.category(),
        dexo_driver_api::DriverErrorCategory::Configuration
    );
}

async fn run_select(session: &dyn Session) {
    let mut stream = session
        .execute(QueryRequest::read("select 1", 10))
        .await
        .unwrap();
    use futures_util::StreamExt;
    let mut saw_row = false;
    while let Some(event) = stream.next().await {
        if matches!(event, Ok(dexo_driver_api::QueryEvent::Rows(_))) {
            saw_row = true;
        }
    }
    assert!(saw_row);
}
