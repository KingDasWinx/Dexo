//! The plugin host: one process shared, started on demand, checked by its handshake,
//! stopped when idle, not restarted forever. Plugins here are threads with pipes.
use std::io::BufReader;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dexo_app::plugin::host::{PluginHost, Spawned, Spawner};
use dexo_driver_sqlite::SqliteFactory;
use dexo_plugin_protocol::{Request, Response, read_line, write_line};
use serde_json::{Value, json};

fn sqlite() -> Spawner {
    Box::new(|| {
        let (reader, writer) = dexo_plugin_sdk::serve_in_process(vec![Arc::new(SqliteFactory)])?;
        Ok(Spawned {
            reader: Box::new(reader),
            writer: Box::new(writer),
            stderr: None,
            child: None,
        })
    })
}

/// A plugin that answers the handshake with `reply`; it then hangs up when `then_exit`,
/// else waits for the host to.
fn fake(reply: Value, then_exit: bool) -> Spawner {
    Box::new(move || {
        let (host_reader, mut plugin_writer) = std::io::pipe()?;
        let (plugin_reader, host_writer) = std::io::pipe()?;
        let reply = reply.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(plugin_reader);
            let mut line = Vec::new();
            if !read_line(&mut reader, &mut line).unwrap_or(false) {
                return;
            }
            let request: Request = serde_json::from_slice(&line).unwrap();
            let _ = write_line(&mut plugin_writer, &Response::ok(request.id, reply));
            if then_exit {
                return;
            }
            while read_line(&mut reader, &mut line).unwrap_or(false) {}
        });
        Ok(Spawned {
            reader: Box::new(host_reader),
            writer: Box::new(host_writer),
            stderr: None,
            child: None,
        })
    })
}

/// A plugin that reads and never answers.
fn silent() -> Spawner {
    Box::new(|| {
        let (host_reader, plugin_writer) = std::io::pipe()?;
        let (plugin_reader, host_writer) = std::io::pipe()?;
        std::thread::spawn(move || {
            let _keep = plugin_writer;
            let mut reader = BufReader::new(plugin_reader);
            let mut line = Vec::new();
            while read_line(&mut reader, &mut line).unwrap_or(false) {}
        });
        Ok(Spawned {
            reader: Box::new(host_reader),
            writer: Box::new(host_writer),
            stderr: None,
            child: None,
        })
    })
}

fn handshake(protocol: u32, drivers: &[&str]) -> Value {
    json!({"protocol": protocol, "driver_version": "1.0.0", "drivers": drivers})
}

#[tokio::test]
async fn the_process_is_started_once_and_shared() {
    let host = PluginHost::new("SQLite", vec!["sqlite".into()], sqlite());
    let first = host.live().await.unwrap();
    let second = host.live().await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn a_handshake_with_another_protocol_is_refused() {
    let host = PluginHost::new(
        "Fake",
        vec!["fake".into()],
        fake(handshake(2, &["fake"]), false),
    );
    let error = host.live().await.err().unwrap();
    assert_eq!(
        error.to_string(),
        "the Fake plugin is broken: it speaks protocol 2, and this Dexo speaks 1"
    );
}

#[tokio::test]
async fn a_handshake_missing_a_driver_is_refused() {
    let host = PluginHost::new(
        "Fake",
        vec!["fake".into()],
        fake(handshake(1, &["other"]), false),
    );
    let error = host.live().await.err().unwrap();
    assert!(
        error
            .to_string()
            .contains("does not provide the fake driver"),
        "{error}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_silent_plugin_fails_its_handshake_after_ten_seconds() {
    let host = PluginHost::new("Fake", vec!["fake".into()], silent());
    let error = host.live().await.err().unwrap();
    assert!(error.to_string().contains("within 10 seconds"), "{error}");
}

#[tokio::test]
async fn a_plugin_that_keeps_crashing_is_restarted_three_times_a_minute_at_most() {
    let host = PluginHost::new(
        "Fake",
        vec!["fake".into()],
        fake(handshake(1, &["fake"]), true),
    );
    for _ in 0..4 {
        let live = host.live().await.unwrap();
        while live.rpc.closed().is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    let error = host.live().await.err().unwrap();
    assert!(
        error.to_string().contains("stopped 3 times in a minute"),
        "{error}"
    );
}

#[tokio::test]
async fn an_idle_plugin_stops_and_starts_again_on_demand() {
    let host = Arc::new(
        PluginHost::new("SQLite", vec!["sqlite".into()], sqlite())
            .with_idle(Duration::from_millis(50)),
    );
    let live = host.live().await.unwrap();
    host.session_opened();
    host.session_closed();
    let started = Instant::now();
    while live.rpc.closed().is_none() {
        assert!(started.elapsed() < Duration::from_secs(5), "never stopped");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Stops on purpose are no crashes: the budget is untouched.
    for _ in 0..4 {
        host.live().await.unwrap();
        host.stop("again");
    }
    assert!(host.live().await.is_ok());
}

#[tokio::test]
async fn a_stopped_plugin_says_why() {
    let host = PluginHost::new("SQLite", vec!["sqlite".into()], sqlite());
    let live = host.live().await.unwrap();
    host.stop("it was removed");
    let error = host
        .ask::<Value>(&live, "catalog/databases", json!({}), None)
        .await
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "the SQLite plugin stopped: it was removed"
    );
}

#[tokio::test]
async fn a_log_line_that_is_not_utf8_or_very_long_does_not_stop_the_log() {
    let mut log = b"caf\xe9 broken\n".to_vec();
    log.extend(std::iter::repeat_n(b'a', 100_000));
    log.extend(b"\nlast words\n");
    let inner = fake(handshake(1, &["fake"]), true);
    let host = PluginHost::new(
        "Fake",
        vec!["fake".into()],
        Box::new(move || {
            let mut spawned = inner()?;
            spawned.stderr = Some(Box::new(std::io::Cursor::new(log.clone())));
            Ok(spawned)
        }),
    );
    let live = host.live().await.unwrap();
    while live.rpc.closed().is_none() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let error = host
        .ask::<Value>(&live, "catalog/databases", json!({}), None)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().ends_with("last words"), "{error}");
}

#[tokio::test]
async fn a_result_answered_after_it_was_given_up_on_is_closed() {
    use dexo_app::plugin::rpc::RpcClient;
    let (host_reader, mut plugin_writer) = std::io::pipe().unwrap();
    let (plugin_reader, host_writer) = std::io::pipe().unwrap();
    let (seen, closes) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(plugin_reader);
        let mut line = Vec::new();
        let mut execute = None;
        while read_line(&mut reader, &mut line).unwrap_or(false) {
            let request: Request = serde_json::from_slice(&line).unwrap();
            match request.method.as_str() {
                "query/execute" => execute = Some(request.id),
                // The answer crosses the cancel: it goes out after it.
                "$/cancel_request" => {
                    let reply = json!({"cursor": "c1", "events": [], "done": false});
                    let _ = write_line(
                        &mut plugin_writer,
                        &Response::ok(execute.take().unwrap(), reply),
                    );
                }
                _ => {
                    let _ = seen.send(json!({"method": request.method, "params": request.params}));
                }
            }
        }
    });
    let rpc = RpcClient::start(host_reader, host_writer);
    let given_up = rpc
        .call::<Value>("query/execute", json!({}), Some(Duration::from_millis(50)))
        .await;
    assert!(given_up.is_err());
    let closed = tokio::task::spawn_blocking(move || closes.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("the late cursor was never closed");
    assert_eq!(
        closed,
        json!({"method": "query/close", "params": {"cursor": "c1"}})
    );
}

#[tokio::test]
async fn a_plugin_that_dies_in_its_handshake_counts_toward_the_restarts() {
    let host = PluginHost::new("Fake", vec!["fake".into()], silent_then_exit());
    let mut last = None;
    for _ in 0..6 {
        last = host.live().await.err();
    }
    let error = last.unwrap();
    assert!(
        error.to_string().contains("stopped 3 times in a minute"),
        "{error}"
    );
}

/// A plugin that exits before it answers anything.
fn silent_then_exit() -> Spawner {
    Box::new(|| {
        let (host_reader, plugin_writer) = std::io::pipe()?;
        let (plugin_reader, host_writer) = std::io::pipe()?;
        drop((plugin_writer, plugin_reader));
        Ok(Spawned {
            reader: Box::new(host_reader),
            writer: Box::new(host_writer),
            stderr: None,
            child: None,
        })
    })
}
