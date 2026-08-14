use std::{collections::HashMap, sync::Mutex};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RuntimeTaskId(pub Uuid);

pub struct TaskHandle {
    pub id: RuntimeTaskId,
    pub token: CancellationToken,
}

#[derive(Default)]
pub struct TaskRegistry {
    // ponytail: process-wide Mutex; shard or DashMap when task volume makes contention measurable
    tokens: Mutex<HashMap<RuntimeTaskId, CancellationToken>>,
}

impl TaskRegistry {
    pub fn register(&self) -> TaskHandle {
        let id = RuntimeTaskId(Uuid::new_v4());
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .expect("task registry poisoned")
            .insert(id, token.clone());
        TaskHandle { id, token }
    }

    pub fn cancel(&self, id: RuntimeTaskId) -> bool {
        self.tokens
            .lock()
            .expect("task registry poisoned")
            .get(&id)
            .map(|token| {
                token.cancel();
                true
            })
            .unwrap_or(false)
    }

    pub fn finish(&self, id: RuntimeTaskId) {
        self.tokens
            .lock()
            .expect("task registry poisoned")
            .remove(&id);
    }
}
