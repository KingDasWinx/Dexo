use dexo_app::transfer::{
    NativeStatus, NativeToolError, NativeToolKind, NativeToolRunner, ProcessRunner, ProcessSpec,
    RunningProcess,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct SlowRunner {
    cancelled: Arc<Mutex<bool>>,
}

struct SlowChild {
    cancelled: Arc<Mutex<bool>>,
}

#[async_trait::async_trait]
impl ProcessRunner for SlowRunner {
    async fn spawn(&self, _: ProcessSpec) -> Result<Box<dyn RunningProcess>, NativeToolError> {
        Ok(Box::new(SlowChild {
            cancelled: Arc::clone(&self.cancelled),
        }))
    }
}

#[async_trait::async_trait]
impl RunningProcess for SlowChild {
    async fn cancel(&mut self) -> Result<(), NativeToolError> {
        *self.cancelled.lock().unwrap() = true;
        Ok(())
    }

    async fn wait(&mut self) -> Result<NativeStatus, NativeToolError> {
        if *self.cancelled.lock().unwrap() {
            Ok(NativeStatus::Cancelled)
        } else {
            Ok(NativeStatus::Succeeded)
        }
    }
}

#[tokio::test]
async fn cancellation_kills_the_child_and_removes_secret_material() {
    let temp = tempfile::tempdir().unwrap();
    let cancelled = Arc::new(Mutex::new(false));
    let runner = NativeToolRunner::new(SlowRunner {
        cancelled: Arc::clone(&cancelled),
    });
    let handle = runner
        .start(NativeToolKind::PgDump, "SECRET", "16.9", 16, temp.path())
        .await
        .unwrap();
    handle.cancel().await.unwrap();
    assert!(!handle.secret_file().exists());
    assert_eq!(
        handle.outcome().await.unwrap().status,
        NativeStatus::Cancelled
    );
    let _ = PathBuf::from(".");
}
