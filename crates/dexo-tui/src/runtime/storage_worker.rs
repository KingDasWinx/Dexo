use dexo_app::{ConnectionProfile, Project, ProjectId};
use dexo_storage::{
    ConnectionRepository, Database, HistoryRepository, LayoutRepository, ProjectRepository,
    SessionRecoveryRepository, SessionRecoveryState, SnippetRepository, WorkbenchLayout,
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
    PersistHistory {
        connection_id: Option<String>,
        sql: String,
    },
    ListHistory {
        connection_id: Option<String>,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Vec<String>>>,
    },
    ClearHistory {
        connection_id: String,
    },
    ListSnippets {
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Vec<dexo_sql::Snippet>>>,
    },
    DeleteSnippet {
        id: String,
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
                        StorageCommand::PersistHistory {
                            connection_id,
                            sql,
                        } => {
                            let repo = HistoryRepository::new(db.connection());
                            let id = Uuid::new_v4().to_string();
                            let _ = repo.insert(&id, connection_id.as_deref(), &sql);
                            let _ = repo.prune(500);
                        }
                        StorageCommand::ListHistory {
                            connection_id,
                            reply,
                        } => {
                            let repo = HistoryRepository::new(db.connection());
                            let result = repo.list(connection_id.as_deref()).map(|rows| {
                                rows.into_iter().map(|(_, sql)| sql).collect()
                            });
                            let _ = reply.send(result);
                        }
                        StorageCommand::ClearHistory { connection_id } => {
                            let repo = HistoryRepository::new(db.connection());
                            let _ = repo.clear_for_connection(&connection_id);
                        }
                        StorageCommand::ListSnippets { reply } => {
                            let repo = SnippetRepository::new(db.connection());
                            let result = repo.list().map(|rows| {
                                rows.into_iter()
                                    .map(|(_, name, body)| dexo_sql::Snippet { name, body })
                                    .collect()
                            });
                            let _ = reply.send(result);
                        }
                        StorageCommand::DeleteSnippet { id } => {
                            let repo = SnippetRepository::new(db.connection());
                            let _ = repo.delete(&id);
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

    pub fn persist_history(
        &self,
        connection_id: Option<String>,
        sql: String,
    ) -> anyhow::Result<()> {
        self.tx
            .send(StorageCommand::PersistHistory { connection_id, sql })?;
        Ok(())
    }

    pub async fn list_history(
        &self,
        connection_id: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageCommand::ListHistory {
                connection_id,
                reply,
            })?;
        receive.await?
    }

    pub fn clear_history(&self, connection_id: String) -> anyhow::Result<()> {
        self.tx
            .send(StorageCommand::ClearHistory { connection_id })?;
        Ok(())
    }

    pub async fn list_snippets(&self) -> anyhow::Result<Vec<dexo_sql::Snippet>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::ListSnippets { reply })?;
        receive.await?
    }

    pub fn delete_snippet(&self, id: String) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::DeleteSnippet { id })?;
        Ok(())
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
