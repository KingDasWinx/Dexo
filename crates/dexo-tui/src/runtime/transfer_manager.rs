use dexo_app::transfer::{ExportError, RecordingSink, export_row_batches};
use dexo_driver_api::DbValue;

pub struct TransferManager;

impl TransferManager {
    pub async fn export_batches(
        batches: impl IntoIterator<Item = Vec<Vec<DbValue>>>,
        sink: RecordingSink,
    ) -> Result<(), ExportError> {
        export_row_batches(batches, sink).await
    }
}
