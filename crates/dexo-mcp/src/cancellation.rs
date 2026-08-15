use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct CancellationRegistry {
    token: CancellationToken,
}

impl Default for CancellationRegistry {
    fn default() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }
}

impl CancellationRegistry {
    pub fn child(&self) -> (Uuid, CancellationToken) {
        (Uuid::new_v4(), self.token.child_token())
    }

    pub fn cancel_all(&self) {
        self.token.cancel();
    }
}
