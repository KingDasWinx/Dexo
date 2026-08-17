use std::path::Path;

use dexo_app::transfer::{NativeHandle, NativeToolRequest, NativeToolRunner, ProcessRunner};

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
        request: NativeToolRequest,
        version: &str,
        dir: &Path,
    ) -> Result<NativeHandle, dexo_app::transfer::native_tool::NativeToolError> {
        self.runner.start(request, version, dir).await
    }
}
