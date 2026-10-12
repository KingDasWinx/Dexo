//! Downloads over HTTPS with the system's certificates, on the calling thread: the TUI
//! calls these on a blocking thread, so the screen never waits on the network.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// No byte for this long, and the server is taken to have stopped answering.
const STALL: Duration = Duration::from_secs(20);

fn agent(limit: u64) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        // GitHub answers a release asset with a redirect to its CDN.
        .max_redirects(10)
        .http_status_as_error(true)
        .timeout_connect(Some(Duration::from_secs(15)))
        // The whole download at 100 KB/s, and a minute: a reader nobody waits for any
        // more still ends.
        .timeout_global(Some(Duration::from_secs(60 + limit / 100_000)))
        .user_agent(concat!("dexo/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// What the reading thread hands over.
enum Got {
    Length(Option<u64>),
    Bytes(Vec<u8>),
    End,
    Failed(String),
}

/// The body at `url`, at most `limit` bytes, in memory: for what is small.
pub fn get(
    url: &str,
    limit: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    get_into(url, limit, &mut body, progress, cancel)?;
    Ok(body)
}

/// The body at `url`, at most `limit` bytes, written to `sink`; its length. `progress`
/// hears the bytes written so far and the total when the server says it; `cancel` stops
/// the download within a tenth of a second, and the error is then `"cancelled"`. A
/// server silent for 20 seconds fails it.
pub fn get_into(
    url: &str,
    limit: u64,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let reading = url.to_string();
    // A blocked read cannot be interrupted: it is left to its thread, which ends with
    // the agent's timeout when nobody waits for it.
    std::thread::spawn(move || read(&reading, limit, &tx));
    let mut written = 0u64;
    let mut total = None;
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Got::Length(length)) => {
                if length.is_some_and(|length| length > limit) {
                    return Err(format!("{url} is larger than the {limit} bytes expected"));
                }
                total = length;
            }
            Ok(Got::Bytes(bytes)) => {
                written += bytes.len() as u64;
                if written > limit {
                    return Err(format!("{url} is larger than the {limit} bytes expected"));
                }
                sink.write_all(&bytes)
                    .map_err(|error| format!("{url} could not be kept: {error}"))?;
                progress(written, total);
            }
            Ok(Got::End) => return Ok(written),
            Ok(Got::Failed(error)) => return Err(error),
            Err(RecvTimeoutError::Timeout) if last.elapsed() < STALL => continue,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("{url} stopped answering"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("{url}: the download stopped"));
            }
        }
        last = Instant::now();
    }
}

fn read(url: &str, limit: u64, tx: &SyncSender<Got>) {
    let response = match agent(limit).get(url).call() {
        Ok(response) => response,
        Err(error) => {
            let _ = tx.send(Got::Failed(describe(url, error)));
            return;
        }
    };
    if tx
        .send(Got::Length(response.body().content_length()))
        .is_err()
    {
        return;
    }
    // ureq refuses a body as long as its limit, not only a longer one: the cap is
    // checked by the caller, one byte past it.
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(limit.saturating_add(1))
        .reader();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let got = match reader.read(&mut chunk) {
            Ok(0) => Got::End,
            Ok(read) => Got::Bytes(chunk[..read].to_vec()),
            Err(error) => Got::Failed(format!("{url}: {error}")),
        };
        let more = matches!(got, Got::Bytes(_));
        if tx.send(got).is_err() || !more {
            return;
        }
    }
}

fn describe(url: &str, error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => format!("{url} was not found"),
        ureq::Error::StatusCode(code) => format!("{url} answered HTTP {code}"),
        other => format!("{url}: {other}"),
    }
}
