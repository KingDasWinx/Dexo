//! The host's end of the plugin protocol: requests out, replies matched back to them by
//! id. Two threads of its own carry the lines, so no async runtime has to outlive the
//! plugin -- the CLI's runtimes come and go.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_plugin_protocol::{self as protocol, Request, Response, RpcError, code};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::oneshot;

use super::lock;

/// Why the conversation ended.
#[derive(Clone, Debug)]
pub struct Closed {
    pub why: String,
    /// The plugin broke the protocol, rather than going away.
    pub violation: bool,
}

#[derive(Debug)]
pub enum CallError {
    /// The plugin answered with an error.
    Remote(RpcError),
    /// The conversation is over.
    Closed(Closed),
    TimedOut,
}

type Reply = Result<Value, CallError>;

/// What the reading thread and the client share.
struct Shared {
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    closed: Mutex<Option<Closed>>,
}

impl Shared {
    /// Ends the conversation: every call waiting fails, and so does every later one.
    /// The first reason given stays.
    fn shut(&self, closed: Closed) {
        let mut slot = lock(&self.closed);
        let closed = slot.get_or_insert(closed).clone();
        for (_, waiting) in lock(&self.pending).drain() {
            let _ = waiting.send(Err(CallError::Closed(closed.clone())));
        }
    }
}

pub struct RpcClient {
    next: AtomicU64,
    shared: Arc<Shared>,
    out: Mutex<Option<std::sync::mpsc::Sender<Vec<u8>>>>,
}

impl RpcClient {
    pub fn start(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared {
            pending: Mutex::default(),
            closed: Mutex::default(),
        });
        let (out, lines) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut writer = writer;
            for line in lines {
                if writer
                    .write_all(&line)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    return;
                }
            }
        });
        let reading = shared.clone();
        std::thread::spawn(move || read_replies(BufReader::new(reader), &reading));
        Arc::new(Self {
            next: AtomicU64::new(1),
            shared,
            out: Mutex::new(Some(out)),
        })
    }

    pub async fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
        timeout: Option<Duration>,
    ) -> Result<R, CallError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let line = encode(id, method, params)?;
        let (sender, receiver) = oneshot::channel();
        {
            // Under the closed lock: a shutdown cannot slip in between the check and
            // the insert and leave this call waiting for good.
            let closed = lock(&self.shared.closed);
            if let Some(closed) = closed.as_ref() {
                return Err(CallError::Closed(closed.clone()));
            }
            lock(&self.shared.pending).insert(id, sender);
        }
        if !self.send(line) {
            self.close(Closed {
                why: "it exited".into(),
                violation: false,
            });
        }
        let reply = match timeout {
            None => receiver.await,
            Some(limit) => match tokio::time::timeout(limit, receiver).await {
                Ok(reply) => reply,
                Err(_) => {
                    lock(&self.shared.pending).remove(&id);
                    return Err(CallError::TimedOut);
                }
            },
        };
        let value = reply.map_err(|_| {
            CallError::Closed(Closed {
                why: "it exited".into(),
                violation: false,
            })
        })??;
        serde_json::from_value(value).map_err(|error| {
            CallError::Remote(RpcError::new(
                code::INTERNAL,
                format!("an answer this Dexo cannot read: {error}"),
            ))
        })
    }

    /// Sends a request whose answer nobody waits for: what a dropped result or session
    /// sends on its way out, outside any runtime.
    pub fn notify(&self, method: &str, params: impl Serialize) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(line) = encode(id, method, params) {
            self.send(line);
        }
    }

    /// Ends the conversation. Waiting calls fail with `closed`, and the plugin's stdin
    /// closes, which a plugin takes as the time to exit.
    pub fn close(&self, closed: Closed) {
        self.shared.shut(closed);
        lock(&self.out).take();
    }

    pub fn closed(&self) -> Option<Closed> {
        lock(&self.shared.closed).clone()
    }

    fn send(&self, line: Vec<u8>) -> bool {
        lock(&self.out)
            .as_ref()
            .is_some_and(|out| out.send(line).is_ok())
    }
}

fn read_replies(mut reader: impl BufRead, shared: &Shared) {
    let mut line = Vec::new();
    let closed = loop {
        match protocol::read_line(&mut reader, &mut line) {
            Ok(false) => {
                break Closed {
                    why: "it exited".into(),
                    violation: false,
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                break Closed {
                    why: format!("it broke the protocol: {error}"),
                    violation: true,
                };
            }
            Err(error) => {
                break Closed {
                    why: format!("its output could not be read: {error}"),
                    violation: false,
                };
            }
            Ok(true) if line.is_empty() => {}
            Ok(true) => match serde_json::from_slice::<Response>(&line) {
                Ok(response) => {
                    let waiting = lock(&shared.pending).remove(&response.id);
                    if let Some(waiting) = waiting {
                        let _ = waiting.send(match response.error {
                            Some(error) => Err(CallError::Remote(error)),
                            None => Ok(response.result.unwrap_or(Value::Null)),
                        });
                    }
                }
                Err(_) => {
                    break Closed {
                        why: "it wrote something that is not the plugin protocol".into(),
                        violation: true,
                    };
                }
            },
        }
    };
    shared.shut(closed);
}

fn encode(id: u64, method: &str, params: impl Serialize) -> Result<Vec<u8>, CallError> {
    let internal = |error: serde_json::Error| {
        CallError::Remote(RpcError::new(code::INTERNAL, error.to_string()))
    };
    let request = Request::new(id, method, params).map_err(internal)?;
    let mut line = serde_json::to_vec(&request).map_err(internal)?;
    line.push(b'\n');
    Ok(line)
}
