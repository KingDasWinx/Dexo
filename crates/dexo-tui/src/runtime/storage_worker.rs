use dexo_app::{ConnectionProfile, Project, ProjectId};
use dexo_storage::{
    ConnectionRepository, Database, LayoutRepository, ProjectRepository, SessionRecoveryRepository,
    SessionRecoveryState, WorkbenchLayout,
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct BootstrapState {
    pub active_project: Project,
    pub connections: Vec<ConnectionProfile>,
    pub recovery: SessionRecoveryState,
    pub layout: Option<WorkbenchLayout>,
}

pub enum StorageCommand {
    Bootstrap {
        reply: tokio::sync::oneshot::Sender<anyhow::Result<BootstrapState>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct StorageWorker {
    tx: std::sync::mpsc::Sender<StorageCommand>,
}

impl StorageWorker {
    pub fn start(path: std::path::PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("dexo-storage".into())
            .spawn(move || {
                let db = Database::open(path).expect("open local Dexo database");
                while let Ok(command) = rx.recv() {
                    match command {
                        StorageCommand::Bootstrap { reply } => {
                            let result = bootstrap_state(&db);
                            let _ = reply.send(result);
                        }
                        StorageCommand::Shutdown => break,
                    }
                }
            })?;
        Ok(Self { tx })
    }

    pub async fn bootstrap(&self) -> anyhow::Result<BootstrapState> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::Bootstrap { reply })?;
        receive.await?
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(StorageCommand::Shutdown);
    }
}

impl Drop for StorageWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn bootstrap_state(db: &Database) -> anyhow::Result<BootstrapState> {
    let conn = db.connection();
    let projects = ProjectRepository::new(conn);
    let listed = projects.list()?;
    let active_project = if listed.is_empty() {
        let project = Project {
            id: ProjectId(Uuid::new_v4()),
            name: "Default".into(),
            created_at: unix_stamp(),
        };
        projects.save(&project)?;
        project
    } else {
        listed
            .iter()
            .find(|project| project.name == "Default")
            .cloned()
            .unwrap_or_else(|| listed.into_iter().next().expect("projects table was non-empty"))
    };
    let project_id = active_project.id.0.to_string();
    let connections = ConnectionRepository::new(conn).list()?;
    let recovery = SessionRecoveryRepository::new(conn).load(&project_id)?;
    let layout = LayoutRepository::new(conn).load(&project_id)?;
    Ok(BootstrapState {
        active_project,
        connections,
        recovery,
        layout,
    })
}

fn unix_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
