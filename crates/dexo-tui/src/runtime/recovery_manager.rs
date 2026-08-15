use dexo_app::recovery_service::RecoveryCheckpoint;

#[derive(Default)]
pub struct RecoveryManager {
    pub candidates: Vec<RecoveryCheckpoint>,
}
