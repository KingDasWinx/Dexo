//! Downloads over HTTPS with the system's certificates, on the calling thread: the TUI
//! calls these on a blocking thread, so the screen never waits on the network.

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

fn agent() -> ureq::Agent {
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
        .user_agent(concat!("dexo/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// The body at `url`, at most `limit` bytes. `progress` hears the bytes read so far and
/// the total when the server says it; `cancel` stops the download between reads, and
/// the error is then `"cancelled"`.
pub fn get(
    url: &str,
    limit: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|error| describe(url, error))?;
    let total = response.body().content_length();
    if total.is_some_and(|total| total > limit) {
        return Err(format!("{url} is larger than the {limit} bytes expected"));
    }
    // ureq refuses a body as long as its limit, not only a longer one: the cap is
    // checked here, one byte past it.
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(limit.saturating_add(1))
        .reader();
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("{url}: {error}"))?;
        if read == 0 {
            return Ok(body);
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() as u64 > limit {
            return Err(format!("{url} is larger than the {limit} bytes expected"));
        }
        progress(body.len() as u64, total);
    }
}

fn describe(url: &str, error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => format!("{url} was not found"),
        ureq::Error::StatusCode(code) => format!("{url} answered HTTP {code}"),
        other => format!("{url}: {other}"),
    }
}
