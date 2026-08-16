use std::sync::atomic::{AtomicU32, Ordering};
use tokio_util::sync::CancellationToken;

pub struct LimitTracker {
    inflight: AtomicU32,
    max_concurrency: u32,
    pub max_rows: u64,
    pub max_bytes: u64,
}

impl LimitTracker {
    pub fn new(max_concurrency: u32, max_rows: u64, max_bytes: u64) -> Self {
        Self {
            inflight: AtomicU32::new(0),
            max_concurrency,
            max_rows,
            max_bytes,
        }
    }

    pub fn try_enter(&self) -> bool {
        loop {
            let current = self.inflight.load(Ordering::SeqCst);
            if current >= self.max_concurrency {
                return false;
            }
            if self
                .inflight
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn leave(&self) {
        self.inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn timeout_token(parent: &CancellationToken) -> CancellationToken {
    parent.child_token()
}
