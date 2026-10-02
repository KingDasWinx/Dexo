use std::sync::Arc;

use dexo_app::CatalogService;
use dexo_driver_api::{CatalogListOptions, ObjectId, QualifiedName, SecurityAdmin, Session};

use crate::action::Action;
use crate::runtime::{OperationId, SessionId};

#[allow(clippy::too_many_arguments)] // ponytail: unpack at the call site; a request struct can wait.
pub async fn load_children(
    session: Arc<dyn Session>,
    parent: Option<ObjectId>,
    driver_parent: Option<ObjectId>,
    operation: OperationId,
    session_id: SessionId,
    generation: u64,
    replace_roots: bool,
    include_system: bool,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(reader) = session.catalog() else {
        let _ = action_tx
            .send(Action::CatalogFailed {
                operation,
                session: session_id.0.to_string(),
                generation,
                parent: parent.clone(),
                message: "catalog capability unavailable".into(),
                retryable: false,
            })
            .await;
        return;
    };
    let options = CatalogListOptions { include_system };
    match CatalogService::list_children(reader, driver_parent.as_ref(), &options).await {
        Ok(list) => {
            let _ = action_tx
                .send(Action::CatalogLoaded {
                    operation,
                    session: session_id.0.to_string(),
                    generation,
                    parent,
                    list,
                    replace_roots,
                })
                .await;
        }
        Err(error) => {
            let _ = action_tx
                .send(Action::CatalogFailed {
                    operation,
                    session: session_id.0.to_string(),
                    generation,
                    parent,
                    message: error.to_string(),
                    retryable: true,
                })
                .await;
        }
    }
}

/// What the connected user may do with `object`. The principal is the user the
/// connection logs in as: it used to be the object's own name, so the inspector of a
/// table `orders` asked about a role `orders`. Without a user there is no one to ask about.
async fn privileges_of(
    security: &dyn SecurityAdmin,
    user: Option<&str>,
    object: &QualifiedName,
) -> Result<Vec<String>, String> {
    let Some(user) = user else {
        return Ok(Vec::new());
    };
    security
        .effective_privileges(
            &QualifiedName::new(None::<String>, None::<String>, user),
            object,
        )
        .await
        .map_err(|error| error.to_string())
}

/// `user` is the one the session logged in as, whose privileges the inspector shows.
pub async fn load_inspector(
    session: Arc<dyn Session>,
    user: Option<String>,
    id: ObjectId,
    generation: u64,
    session_id: SessionId,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(reader) = session.catalog() else {
        let _ = action_tx
            .send(Action::InspectorFailed {
                generation,
                message: "catalog capability unavailable".into(),
            })
            .await;
        return;
    };
    let object = CatalogService::object(reader, &id).await.ok().flatten();
    let ddl = CatalogService::ddl(reader, &id)
        .await
        .ok()
        .map(|ddl| ddl.sql);
    let (dependencies, dep_err) = match reader.dependencies(&id).await {
        Ok(ids) => (ids, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let (dependents, dependent_err) = match reader.dependents(&id).await {
        Ok(ids) => (ids, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let mut privileges = Vec::new();
    let mut restrictions = Vec::new();
    if let Some(message) = dep_err {
        restrictions.push(message);
    }
    if let Some(message) = dependent_err {
        restrictions.push(message);
    }
    if let (Some(object), Some(security)) = (object.as_ref(), session.security()) {
        match privileges_of(security, user.as_deref(), &object.qualified_name).await {
            Ok(values) => privileges = values,
            Err(error) => restrictions.push(error),
        }
    }
    let _ = action_tx
        .send(Action::InspectorLoaded {
            generation,
            session: session_id.0.to_string(),
            qualified_name: object
                .as_ref()
                .map(|object| object.qualified_name.display_unquoted())
                .unwrap_or_default(),
            object,
            ddl,
            dependencies,
            dependents,
            effective_privileges: privileges,
            restrictions,
        })
        .await;
}

pub async fn capture_snapshot(
    session: Arc<dyn Session>,
    connection_id: String,
    database_name: String,
    include_system: bool,
    db_path: Option<std::path::PathBuf>,
    generation: u64,
    action_tx: tokio::sync::mpsc::Sender<Action>,
) {
    let Some(reader) = session.catalog() else {
        return;
    };
    // ponytail: collect the walk in memory then one atomic replace_snapshot.
    // Ceiling: huge catalogs. Stream into complete=0 rows if RSS becomes a problem.
    let mut objects = Vec::new();
    let mut queue: Vec<Option<ObjectId>> = vec![None];
    let options = CatalogListOptions { include_system };
    while let Some(parent) = queue.pop() {
        let Ok(list) = CatalogService::list_children(reader, parent.as_ref(), &options).await
        else {
            // ponytail: abort the whole capture so the previous complete=1 snapshot stays active.
            return;
        };
        for object in list.objects {
            queue.push(Some(object.id.clone()));
            objects.push(object);
        }
    }
    if let Some(db_path) = db_path
        && let Ok(db) = dexo_storage::Database::open(&db_path)
    {
        let _ = dexo_storage::CatalogCache::new(db.connection()).replace_snapshot(
            &connection_id,
            &database_name,
            &objects,
        );
    }
    let _ = action_tx
        .send(Action::CompletionCatalogLoaded {
            generation,
            objects,
            complete: true,
        })
        .await;
}
