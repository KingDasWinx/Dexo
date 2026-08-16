use std::path::Path;

use dexo_app::transfer::{NativeHandle, NativeToolKind, NativeToolRunner, ProcessRunner};

pub struct NativeToolManager<R: ProcessRunner> {
    runner: NativeToolRunner<R>,
}

impl<R: ProcessRunner> NativeToolManager<R> {
    pub fn new(process: R) -> Self {
        Self {
            runner: NativeToolRunner::new(process),
        }
    }

    pub async fn start(
        &self,
        kind: NativeToolKind,
        secret: &str,
        version: &str,
        expected_major: u32,
        dir: &Path,
    ) -> Result<NativeHandle, dexo_app::transfer::native_tool::NativeToolError> {
        self.runner
            .start(kind, secret, version, expected_major, dir)
            .await
    }
}
