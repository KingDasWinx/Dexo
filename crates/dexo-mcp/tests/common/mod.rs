#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::{
    Effect, McpConnection, McpProfile, McpService, MemoryGrantLedger, QueryMode, SelectorRule,
};
use dexo_app::schema_diff::SchemaSnapshot;
use dexo_app::{AppError, Environment, ErrorCategory};
use dexo_driver_api::{CatalogObject, Session};
use dexo_mcp::McpBackend;
use dexo_sql::Dialect;
use dexo_test_support::FakeSession;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

#[derive(Default)]
pub struct FakeBackend {
    pub sessions: BTreeMap<String, FakeSession>,
    pub catalog: Vec<CatalogObject>,
    pub snapshots: BTreeMap<String, SchemaSnapshot>,
    pub connects: Mutex<Vec<String>>,
    pub fail_next_connect: AtomicBool,
}

impl FakeBackend {
    pub fn with_session(name: &str, session: FakeSession) -> Self {
        let mut backend = Self::default();
        backend.sessions.insert(name.into(), session);
        backend
    }
}

#[async_trait::async_trait]
impl McpBackend for FakeBackend {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError> {
        self.connects.lock().unwrap().push(connection.into());
        if self.fail_next_connect.swap(false, Ordering::SeqCst) {
            return Err(AppError::new(ErrorCategory::Network, "connection refused"));
        }
        self.sessions
            .get(connection)
            .cloned()
            .map(|session| Box::new(session) as Box<dyn Session>)
            .ok_or_else(|| AppError::new(ErrorCategory::Network, "connection refused"))
    }

    async fn catalog_snapshot(&self, _connection: &str) -> Result<Vec<CatalogObject>, AppError> {
        Ok(self.catalog.clone())
    }

    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError> {
        Ok(self.snapshots.get(name).cloned())
    }
}

pub fn connection(name: &str) -> McpConnection {
    McpConnection {
        name: name.into(),
        driver: "postgres".into(),
        dialect: Dialect::Postgres,
        database: Some("db".into()),
        default_schema: Some("public".into()),
        environment: Environment::Local,
        read_only: false,
    }
}

/// Enabled, raw reads on, `db.public.*` allowed except `db.public.secrets`.
pub fn profile() -> McpProfile {
    let mut profile = McpProfile::new("assistant");
    profile.enabled = true;
    profile.query_mode = QueryMode::RawReadSql;
    profile.connections = vec!["local".into()];
    profile.selectors = vec![
        SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
        SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
    ];
    profile
}

pub struct Client {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    write: WriteHalf<DuplexStream>,
    next_id: u64,
    pub notifications: Vec<Value>,
}

impl Client {
    pub async fn start(
        profile: McpProfile,
        connections: Vec<McpConnection>,
        backend: Arc<FakeBackend>,
        ledger: Arc<MemoryGrantLedger>,
    ) -> Self {
        let (client, server) = tokio::io::duplex(1 << 20);
        let (server_read, server_write) = tokio::io::split(server);
        tokio::spawn(dexo_mcp::serve_io(
            McpService::new(profile),
            connections,
            backend,
            ledger,
            server_read,
            server_write,
        ));
        let (read, write) = tokio::io::split(client);
        let mut client = Self {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 0,
            notifications: Vec::new(),
        };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"}
                }),
            )
            .await;
        assert!(init.get("result").is_some(), "{init}");
        client.notify("notifications/initialized", json!({})).await;
        client
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await;
    }

    pub async fn send_request(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        id
    }

    /// Every line the server writes must be one JSON-RPC 2.0 message (MCP-001).
    pub async fn read_message(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line())
            .await
            .expect("the server answered in time")
            .unwrap()
            .expect("the server is still open");
        let message: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
        assert_eq!(message["jsonrpc"], "2.0", "{message}");
        message
    }

    pub async fn response(&mut self, id: u64) -> Value {
        loop {
            let message = self.read_message().await;
            if message["id"] == json!(id) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_request(method, params).await;
        self.response(id).await
    }

    pub async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await["result"]
            .clone()
    }

    pub async fn tools(&mut self) -> Vec<Value> {
        self.request("tools/list", json!({})).await["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub async fn tool_names(&mut self) -> Vec<String> {
        self.tools()
            .await
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    /// Waits up to three seconds for a notification with this method.
    pub async fn saw_notification(&mut self, method: &str) -> bool {
        if self
            .notifications
            .iter()
            .any(|message| message["method"] == method)
        {
            return true;
        }
        for _ in 0..20 {
            let next =
                tokio::time::timeout(Duration::from_millis(150), self.lines.next_line()).await;
            if let Ok(Ok(Some(line))) = next {
                let message: Value = serde_json::from_str(&line).unwrap();
                if message["method"] == method {
                    return true;
                }
                self.notifications.push(message);
            }
        }
        false
    }

    async fn send(&mut self, message: Value) {
        self.write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.write.flush().await.unwrap();
    }
}

pub fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

pub fn is_error(result: &Value) -> bool {
    result["isError"] == json!(true)
}
