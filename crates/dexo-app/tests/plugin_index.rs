//! The plugin index: signed or refused, kept a day, used offline, fetched again when
//! the cache is damaged; and downloads that stop at their cap.
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use dexo_app::plugin::http;
use dexo_app::plugin::index::{self, IndexSource};
use dexo_test_support::plugins::{HttpServer, TestKey};

const EMPTY: &str = r#"{"schema":1,"generated":"2026-10-06T00:00:00Z","plugins":[]}"#;

fn served(index: &str) -> (HttpServer, IndexSource) {
    let server = HttpServer::start();
    let key = TestKey::new();
    server.put("/index.json", index);
    server.put("/index.json.minisig", key.sign(index.as_bytes()));
    let source = IndexSource {
        url: server.url("/index.json"),
        keys: vec![key.public()],
    };
    (server, source)
}

#[test]
fn a_signed_index_loads_and_is_kept_a_day() {
    let data = tempfile::tempdir().unwrap();
    let (server, source) = served(EMPTY);
    let fetched = index::load(data.path(), &source, false).unwrap();
    assert!(!fetched.offline);
    assert!(data.path().join("plugins/index-cache.json").is_file());
    server.put("/index.json", "changed on the server");
    let cached = index::load(data.path(), &source, false).unwrap();
    assert_eq!(cached.index, fetched.index);
}

#[test]
fn a_signature_that_does_not_match_installs_nothing() {
    let data = tempfile::tempdir().unwrap();
    let (_server, mut source) = served(EMPTY);
    source.keys = vec![TestKey::new().public()];
    let error = index::load(data.path(), &source, false).err().unwrap();
    assert_eq!(
        error.to_string(),
        "the index signature does not match: nothing was installed"
    );
    assert!(!data.path().join("plugins/index-cache.json").exists());
}

#[test]
fn offline_the_cache_is_used_and_said_to_be() {
    let data = tempfile::tempdir().unwrap();
    let (_server, source) = served(EMPTY);
    index::load(data.path(), &source, false).unwrap();
    let cache = data.path().join("plugins/index-cache.json");
    let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
    std::fs::File::options()
        .write(true)
        .open(&cache)
        .unwrap()
        .set_modified(two_days_ago)
        .unwrap();
    let offline = IndexSource {
        url: HttpServer::dead_url(),
        keys: source.keys.clone(),
    };
    let fetched = index::load(data.path(), &offline, false).unwrap();
    assert!(fetched.offline);
    assert!(
        index::offline_note(fetched.fetched_at).starts_with("offline: showing the index from 20")
    );
    assert!(index::load(tempfile::tempdir().unwrap().path(), &offline, false).is_err());
}

/// Review focus 4.
#[test]
fn a_corrupt_cache_is_fetched_again() {
    for corrupt in ["", "{\"schema\":1,\"plugins\":[", EMPTY] {
        let data = tempfile::tempdir().unwrap();
        let (_server, source) = served(EMPTY);
        let plugins = data.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("index-cache.json"), corrupt).unwrap();
        std::fs::write(
            plugins.join("index-cache.json.minisig"),
            "untrusted comment: tampered\nnot a signature\n",
        )
        .unwrap();
        let fetched = index::load(data.path(), &source, false).unwrap();
        assert!(!fetched.offline);
        assert_eq!(
            std::fs::read_to_string(plugins.join("index-cache.json")).unwrap(),
            EMPTY
        );
    }
}

#[test]
fn a_download_stops_at_its_cap_and_when_cancelled() {
    let server = HttpServer::start();
    server.put("/big", vec![7u8; 100_000]);
    let never = AtomicBool::new(false);
    let mut seen = (0, None);
    let body = http::get(
        &server.url("/big"),
        1_000_000,
        &mut |done, total| seen = (done, total),
        &never,
    )
    .unwrap();
    assert_eq!(body.len(), 100_000);
    assert_eq!(seen, (100_000, Some(100_000)));
    let capped = http::get(&server.url("/big"), 1_000, &mut |_, _| {}, &never).unwrap_err();
    assert!(capped.contains("larger than"), "{capped}");
    let cancel = AtomicBool::new(true);
    assert_eq!(
        http::get(&server.url("/big"), 1_000_000, &mut |_, _| {}, &cancel).unwrap_err(),
        "cancelled"
    );
    let missing = http::get(&server.url("/missing"), 1_000, &mut |_, _| {}, &never).unwrap_err();
    assert!(missing.contains("was not found"), "{missing}");
}

#[test]
fn a_server_that_never_answers_is_offline() {
    let data = tempfile::tempdir().unwrap();
    let (_server, source) = served(EMPTY);
    index::load(data.path(), &source, false).unwrap();
    let (_silent, url) = HttpServer::silent();
    let stalled = IndexSource {
        url,
        keys: source.keys.clone(),
    };
    let started = std::time::Instant::now();
    let fetched = index::load(data.path(), &stalled, true).unwrap();
    assert!(fetched.offline);
    assert!(started.elapsed() < Duration::from_secs(60));
}

#[test]
fn a_stalled_download_is_cancelled_at_once() {
    let (_silent, url) = HttpServer::silent();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let started = std::time::Instant::now();
    let error = http::get(&url, 1024, &mut |_, _| {}, &cancel).unwrap_err();
    assert_eq!(error, "cancelled");
    assert!(started.elapsed() < Duration::from_secs(3));
}
