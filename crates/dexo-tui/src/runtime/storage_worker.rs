use std::collections::HashMap;
use std::path::PathBuf;

use dexo_app::{ConnectionProfile, Project, ProjectId};
use dexo_storage::{
    ConnectionRepository, Database, DocumentRepository, HistoryRepository, ImportResolution,
    LayoutRepository, ProjectDeletePreview, ProjectRepository, RecentItemsRepository,
    RecoveryRepository, SessionRecoveryRepository, SessionRecoveryState, SnippetRepository,
    StoredDocument, WorkbenchLayout, export_portable, import_portable_resolved, preview_import,
};
use uuid::Uuid;

use crate::action::{DocumentIoRequest, FlushedDocument, RecoveryCheckpointRequest};

#[derive(Clone, Debug, PartialEq)]
pub struct BootstrapState {
    pub active_project: Project,
    pub connections: Vec<ConnectionProfile>,
    pub recovery: SessionRecoveryState,
    pub layout: Option<WorkbenchLayout>,
    pub documents: Vec<StoredDocument>,
    pub projects: Vec<Project>,
    pub snippets: Vec<dexo_sql::Snippet>,
}

#[derive(Clone, Debug)]
pub struct LoadedProject {
    pub project: Project,
    pub documents: Vec<StoredDocument>,
    pub layout: Option<WorkbenchLayout>,
}

pub enum StorageCommand {
    Bootstrap {
        reply: tokio::sync::oneshot::Sender<anyhow::Result<BootstrapState>>,
    },
    PersistHistory {
        project_id: Option<String>,
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
    CheckpointRecovery(RecoveryCheckpointRequest),
    PersistLayout {
        project_id: String,
        layout: WorkbenchLayout,
        reply: Option<tokio::sync::oneshot::Sender<anyhow::Result<()>>>,
    },
    SaveDocument(DocumentIoRequest),
    FlushDocuments {
        project_id: String,
        documents: Vec<FlushedDocument>,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    },
    ListProjects {
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Vec<Project>>>,
    },
    CreateProject {
        name: String,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Vec<Project>>>,
    },
    RenameProject {
        id: String,
        name: String,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Vec<Project>>>,
    },
    DeleteProject {
        id: String,
        delete_connections: bool,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<String>>,
    },
    PreviewDelete {
        id: String,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<(Project, ProjectDeletePreview)>>,
    },
    LoadProject {
        id: String,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<LoadedProject>>,
    },
    GetProjectByName {
        name: String,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<Option<Project>>>,
    },
    ExportConfig {
        path: PathBuf,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    },
    PreviewImport {
        path: PathBuf,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<dexo_storage::ImportPreview>>,
    },
    ApplyImport {
        path: PathBuf,
        resolutions: HashMap<String, ImportResolution>,
        reply: tokio::sync::oneshot::Sender<anyhow::Result<dexo_storage::ImportReport>>,
    },
    MarkCleanShutdown,
    Shutdown,
}

#[derive(Clone)]
pub struct StorageWorker {
    tx: std::sync::mpsc::Sender<StorageCommand>,
}

impl StorageWorker {
    pub fn start(path: PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("dexo-storage".into())
            .spawn(move || {
                let db = Database::open(path).expect("open local Dexo database");
                while let Ok(command) = rx.recv() {
                    match command {
                        StorageCommand::Bootstrap { reply } => {
                            let _ = reply.send(bootstrap_state(&db));
                        }
                        StorageCommand::PersistHistory {
                            project_id,
                            connection_id,
                            sql,
                        } => {
                            let repo = HistoryRepository::new(db.connection());
                            let id = Uuid::new_v4().to_string();
                            let _ = repo.insert_scoped(
                                &id,
                                project_id.as_deref(),
                                connection_id.as_deref(),
                                &sql,
                            );
                            let _ = repo.prune(500);
                        }
                        StorageCommand::ListHistory {
                            connection_id,
                            reply,
                        } => {
                            let repo = HistoryRepository::new(db.connection());
                            let result = repo
                                .list(connection_id.as_deref())
                                .map(|rows| rows.into_iter().map(|(_, sql)| sql).collect());
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
                        StorageCommand::CheckpointRecovery(request) => {
                            let repo = RecoveryRepository::new(db.connection());
                            let _ = repo.checkpoint(
                                &request.document,
                                &request.project_id,
                                &request.title,
                                &request.content,
                            );
                        }
                        StorageCommand::PersistLayout {
                            project_id,
                            layout,
                            reply,
                        } => {
                            let result = (|| {
                                LayoutRepository::new(db.connection())
                                    .save(&project_id, &layout)?;
                                SessionRecoveryRepository::new(db.connection())
                                    .checkpoint_layout(&layout, "idle")
                            })();
                            if let Some(reply) = reply {
                                let _ = reply.send(result);
                            }
                        }
                        StorageCommand::SaveDocument(request) => {
                            let repo = DocumentRepository::new(db.connection());
                            if repo
                                .save(
                                    &request.document,
                                    None,
                                    &request.document,
                                    &request.content,
                                    Some(request.path.to_string_lossy().as_ref()),
                                    None,
                                )
                                .is_ok()
                            {
                                let _ = RecoveryRepository::new(db.connection())
                                    .clear(&request.document);
                            }
                        }
                        StorageCommand::FlushDocuments {
                            project_id,
                            documents,
                            reply,
                        } => {
                            let _ = reply.send(flush_documents(&db, &project_id, &documents));
                        }
                        StorageCommand::ListProjects { reply } => {
                            let _ = reply.send(ProjectRepository::new(db.connection()).list());
                        }
                        StorageCommand::CreateProject { name, reply } => {
                            let repo = ProjectRepository::new(db.connection());
                            let result = repo.create(&name).and_then(|_| repo.list());
                            let _ = reply.send(result);
                        }
                        StorageCommand::RenameProject { id, name, reply } => {
                            let repo = ProjectRepository::new(db.connection());
                            let result = parse_project_id(&id).and_then(|pid| {
                                repo.rename(pid, &name)?;
                                repo.list()
                            });
                            let _ = reply.send(result);
                        }
                        StorageCommand::DeleteProject {
                            id,
                            delete_connections,
                            reply,
                        } => {
                            let _ = reply.send(delete_project(&db, &id, delete_connections));
                        }
                        StorageCommand::PreviewDelete { id, reply } => {
                            let _ = reply.send(preview_delete(&db, &id));
                        }
                        StorageCommand::LoadProject { id, reply } => {
                            let _ = reply.send(load_project(&db, &id));
                        }
                        StorageCommand::GetProjectByName { name, reply } => {
                            let _ = reply
                                .send(ProjectRepository::new(db.connection()).get_by_name(&name));
                        }
                        StorageCommand::ExportConfig { path, reply } => {
                            let result = export_portable(db.connection()).and_then(|toml| {
                                if let Some(parent) = path.parent() {
                                    std::fs::create_dir_all(parent)?;
                                }
                                let tmp = path.with_extension("toml.tmp");
                                std::fs::write(&tmp, toml)?;
                                std::fs::rename(&tmp, &path)?;
                                Ok(())
                            });
                            let _ = reply.send(result);
                        }
                        StorageCommand::PreviewImport { path, reply } => {
                            let result = std::fs::read_to_string(&path)
                                .map_err(Into::into)
                                .and_then(|toml| preview_import(db.connection(), &toml));
                            let _ = reply.send(result);
                        }
                        StorageCommand::ApplyImport {
                            path,
                            resolutions,
                            reply,
                        } => {
                            let result = std::fs::read_to_string(&path)
                                .map_err(Into::into)
                                .and_then(|toml| {
                                    import_portable_resolved(db.connection(), &toml, &resolutions)
                                });
                            let _ = reply.send(result);
                        }
                        StorageCommand::MarkCleanShutdown => {
                            let _ = SessionRecoveryRepository::new(db.connection())
                                .mark_clean_shutdown();
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
        project_id: Option<String>,
        connection_id: Option<String>,
        sql: String,
    ) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::PersistHistory {
            project_id,
            connection_id,
            sql,
        })?;
        Ok(())
    }

    pub async fn list_history(&self, connection_id: Option<String>) -> anyhow::Result<Vec<String>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::ListHistory {
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

    pub fn checkpoint_recovery(&self, request: RecoveryCheckpointRequest) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::CheckpointRecovery(request))?;
        Ok(())
    }

    pub fn persist_layout(
        &self,
        project_id: String,
        layout: WorkbenchLayout,
    ) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::PersistLayout {
            project_id,
            layout,
            reply: None,
        })?;
        Ok(())
    }

    pub async fn persist_layout_wait(
        &self,
        project_id: String,
        layout: WorkbenchLayout,
    ) -> anyhow::Result<()> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::PersistLayout {
            project_id,
            layout,
            reply: Some(reply),
        })?;
        receive.await?
    }

    pub fn save_document(&self, request: DocumentIoRequest) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::SaveDocument(request))?;
        Ok(())
    }

    pub async fn flush_documents(
        &self,
        project_id: String,
        documents: Vec<FlushedDocument>,
    ) -> anyhow::Result<()> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::FlushDocuments {
            project_id,
            documents,
            reply,
        })?;
        receive.await?
    }

    pub async fn list_projects(&self) -> anyhow::Result<Vec<Project>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::ListProjects { reply })?;
        receive.await?
    }

    pub async fn create_project(&self, name: String) -> anyhow::Result<Vec<Project>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageCommand::CreateProject { name, reply })?;
        receive.await?
    }

    pub async fn rename_project(&self, id: String, name: String) -> anyhow::Result<Vec<Project>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageCommand::RenameProject { id, name, reply })?;
        receive.await?
    }

    pub async fn delete_project(
        &self,
        id: String,
        delete_connections: bool,
    ) -> anyhow::Result<String> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::DeleteProject {
            id,
            delete_connections,
            reply,
        })?;
        receive.await?
    }

    pub async fn preview_delete(
        &self,
        id: String,
    ) -> anyhow::Result<(Project, ProjectDeletePreview)> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::PreviewDelete { id, reply })?;
        receive.await?
    }

    pub async fn load_project(&self, id: String) -> anyhow::Result<LoadedProject> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::LoadProject { id, reply })?;
        receive.await?
    }

    pub async fn get_project_by_name(&self, name: String) -> anyhow::Result<Option<Project>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageCommand::GetProjectByName { name, reply })?;
        receive.await?
    }

    pub async fn export_config(&self, path: PathBuf) -> anyhow::Result<()> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::ExportConfig { path, reply })?;
        receive.await?
    }

    pub async fn preview_import(
        &self,
        path: PathBuf,
    ) -> anyhow::Result<dexo_storage::ImportPreview> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageCommand::PreviewImport { path, reply })?;
        receive.await?
    }

    pub async fn apply_import(
        &self,
        path: PathBuf,
        resolutions: HashMap<String, ImportResolution>,
    ) -> anyhow::Result<dexo_storage::ImportReport> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::ApplyImport {
            path,
            resolutions,
            reply,
        })?;
        receive.await?
    }

    pub fn mark_clean_shutdown(&self) -> anyhow::Result<()> {
        self.tx.send(StorageCommand::MarkCleanShutdown)?;
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
            .unwrap_or_else(|| listed[0].clone())
    };
    let project_id = active_project.id.0.to_string();
    let connections = ConnectionRepository::new(conn).list()?;
    let recovery = SessionRecoveryRepository::new(conn).load(&project_id)?;
    SessionRecoveryRepository::new(conn).mark_running()?;
    let layout = LayoutRepository::new(conn).load(&project_id)?;
    let documents = DocumentRepository::new(conn).list_for_project(&project_id)?;
    let snippets = SnippetRepository::new(conn).list().map(|rows| {
        rows.into_iter()
            .map(|(_, name, body)| dexo_sql::Snippet { name, body })
            .collect()
    })?;
    let _ = RecentItemsRepository::new(conn).touch(&project_id, "project", &active_project.name);
    Ok(BootstrapState {
        projects: ProjectRepository::new(conn).list()?,
        active_project,
        connections,
        recovery,
        layout,
        documents,
        snippets,
    })
}

fn flush_documents(
    db: &Database,
    project_id: &str,
    documents: &[FlushedDocument],
) -> anyhow::Result<()> {
    let repo = DocumentRepository::new(db.connection());
    for document in documents {
        let path = document
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        repo.save(
            &document.id,
            Some(project_id),
            &document.title,
            &document.content,
            path.as_deref(),
            None,
        )?;
    }
    Ok(())
}

fn parse_project_id(id: &str) -> anyhow::Result<ProjectId> {
    Ok(ProjectId(Uuid::parse_str(id)?))
}

fn preview_delete(db: &Database, id: &str) -> anyhow::Result<(Project, ProjectDeletePreview)> {
    let pid = parse_project_id(id)?;
    let repo = ProjectRepository::new(db.connection());
    let project = repo
        .get(pid)?
        .ok_or_else(|| anyhow::anyhow!("unknown project {id}"))?;
    Ok((project, repo.preview_delete(pid)?))
}

fn delete_project(db: &Database, id: &str, delete_connections: bool) -> anyhow::Result<String> {
    let pid = parse_project_id(id)?;
    let repo = ProjectRepository::new(db.connection());
    let project = repo
        .get(pid)?
        .ok_or_else(|| anyhow::anyhow!("unknown project {id}"))?;
    if delete_connections {
        let connections = ConnectionRepository::new(db.connection()).list_for_project(pid.0)?;
        for profile in connections {
            ConnectionRepository::new(db.connection()).delete(profile.id)?;
        }
    }
    repo.delete(pid)?;
    Ok(project.name)
}

fn load_project(db: &Database, id: &str) -> anyhow::Result<LoadedProject> {
    let pid = parse_project_id(id)?;
    let project = ProjectRepository::new(db.connection())
        .get(pid)?
        .ok_or_else(|| anyhow::anyhow!("unknown project {id}"))?;
    let project_id = pid.0.to_string();
    let documents = DocumentRepository::new(db.connection()).list_for_project(&project_id)?;
    let layout = LayoutRepository::new(db.connection()).load(&project_id)?;
    let _ =
        RecentItemsRepository::new(db.connection()).touch(&project_id, "project", &project.name);
    Ok(LoadedProject {
        project,
        documents,
        layout,
    })
}

fn unix_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
