//! The families beyond queries: each maps a method onto the driver trait it names.

use dexo_driver_api::{
    Capability, CatalogListOptions, DataRequest, DbValue, DriverError, ExplainRequest, Mutation,
    ObjectId, QualifiedName, RemoteValueRef, Session, TransactionMode,
};
use dexo_plugin_protocol::{Bytes, Family, OnSession, RpcError, TxResult, code, method};
use serde_json::Value;

use crate::{Server, parse, reply};

/// The families `session` answers.
pub(crate) fn of(session: &dyn Session) -> Vec<Family> {
    let mut families = Vec::new();
    if session.catalog().is_some() {
        families.push(Family::Catalog);
    }
    if session.data().is_some() {
        families.push(Family::Data);
    }
    if session.bulk().is_some() {
        families.push(Family::Bulk);
    }
    if session.explain().is_some() {
        families.push(Family::Explain);
    }
    if session.transactions().is_some() {
        families.push(Family::Transactions);
    }
    if session
        .capabilities()
        .iter()
        .any(|state| state.capability == Capability::Cancel && state.available)
    {
        families.push(Family::Cancel);
    }
    families
}

pub(crate) async fn dispatch(
    server: &Server,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    let family = method.split('/').next().unwrap_or_default();
    if !matches!(family, "catalog" | "data" | "bulk" | "explain" | "tx") {
        return Err(not_found(method));
    }
    let call: OnSession<Value> = parse(params)?;
    let session = server.session(&call.session)?;
    let session: &dyn Session = &**session;
    match family {
        "catalog" => catalog(session, method, call.args).await,
        "data" => data(session, method, call.args).await,
        "bulk" => bulk(session, call.args).await,
        "explain" => explain(session, call.args).await,
        _ => transactions(session, method, call.args).await,
    }
}

fn not_found(method: &str) -> RpcError {
    RpcError::new(code::METHOD_NOT_FOUND, format!("no method '{method}'"))
}

fn missing(family: &str) -> RpcError {
    RpcError::new(
        code::METHOD_NOT_FOUND,
        format!("this session has no {family} methods"),
    )
}

fn driver(error: DriverError) -> RpcError {
    RpcError::driver(&error)
}

async fn catalog(session: &dyn Session, name: &str, args: Value) -> Result<Value, RpcError> {
    let reader = session.catalog().ok_or_else(|| missing("catalog"))?;
    match name {
        method::CATALOG_LIST_CHILDREN => {
            let (parent, options): (Option<ObjectId>, CatalogListOptions) = parse(args)?;
            reply(
                reader
                    .list_children(parent.as_ref(), &options)
                    .await
                    .map_err(driver)?,
            )
        }
        method::CATALOG_OBJECT => {
            let id: ObjectId = parse(args)?;
            reply(reader.object(&id).await.map_err(driver)?)
        }
        method::CATALOG_DDL => {
            let id: ObjectId = parse(args)?;
            reply(reader.ddl(&id).await.map_err(driver)?)
        }
        method::CATALOG_DEPENDENCIES => {
            let id: ObjectId = parse(args)?;
            reply(reader.dependencies(&id).await.map_err(driver)?)
        }
        method::CATALOG_DEPENDENTS => {
            let id: ObjectId = parse(args)?;
            reply(reader.dependents(&id).await.map_err(driver)?)
        }
        method::CATALOG_FOREIGN_KEYS => {
            let table: QualifiedName = parse(args)?;
            reply(reader.foreign_keys(&table).await.map_err(driver)?)
        }
        method::CATALOG_RELATIONS_NAMED => {
            let (schema, name): (Option<String>, String) = parse(args)?;
            reply(
                reader
                    .relations_named(schema.as_deref(), &name)
                    .await
                    .map_err(driver)?,
            )
        }
        method::CATALOG_DATABASES => reply(reader.databases().await.map_err(driver)?),
        other => Err(not_found(other)),
    }
}

async fn data(session: &dyn Session, name: &str, args: Value) -> Result<Value, RpcError> {
    let mutator = session.data().ok_or_else(|| missing("data"))?;
    match name {
        method::DATA_FETCH => {
            let request: DataRequest = parse(args)?;
            reply(mutator.fetch(request).await.map_err(driver)?)
        }
        method::DATA_FETCH_VALUE => {
            let (value, offset, limit): (RemoteValueRef, u64, u32) = parse(args)?;
            reply(Bytes(
                mutator
                    .fetch_value(&value, offset, limit)
                    .await
                    .map_err(driver)?,
            ))
        }
        method::DATA_APPLY => {
            let mutations: Vec<Mutation> = parse(args)?;
            reply(mutator.apply(&mutations).await.map_err(driver)?)
        }
        method::DATA_TABLE_COLUMNS => {
            let table: QualifiedName = parse(args)?;
            reply(mutator.table_columns(&table).await.map_err(driver)?)
        }
        method::DATA_ESTIMATE_ROWS => {
            let table: QualifiedName = parse(args)?;
            reply(mutator.estimate_rows(&table).await.map_err(driver)?)
        }
        other => Err(not_found(other)),
    }
}

async fn bulk(session: &dyn Session, args: Value) -> Result<Value, RpcError> {
    let writer = session.bulk().ok_or_else(|| missing("bulk"))?;
    let (table, columns, rows): (QualifiedName, Vec<String>, Vec<Vec<DbValue>>) = parse(args)?;
    reply(
        writer
            .insert_batch(&table, &columns, &rows)
            .await
            .map_err(driver)?,
    )
}

async fn explain(session: &dyn Session, args: Value) -> Result<Value, RpcError> {
    let provider = session.explain().ok_or_else(|| missing("explain"))?;
    let request: ExplainRequest = parse(args)?;
    reply(provider.explain(request).await.map_err(driver)?)
}

/// Every transaction method answers the state it left, failed or not: the host's
/// `state()` is synchronous and reads what the last answer said.
async fn transactions(session: &dyn Session, name: &str, args: Value) -> Result<Value, RpcError> {
    let control = session
        .transactions()
        .ok_or_else(|| missing("transactions"))?;
    let result = match name {
        method::TX_BEGIN => {
            let mode: TransactionMode = parse(args)?;
            control.begin(mode).await
        }
        method::TX_COMMIT => control.commit().await,
        method::TX_ROLLBACK => control.rollback().await,
        method::TX_SAVEPOINT => control.savepoint(&parse::<String>(args)?).await,
        method::TX_ROLLBACK_TO => control.rollback_to(&parse::<String>(args)?).await,
        method::TX_RELEASE_SAVEPOINT => control.release_savepoint(&parse::<String>(args)?).await,
        other => return Err(not_found(other)),
    };
    reply(TxResult {
        state: control.state(),
        notice: control.take_notice(),
        error: result.err(),
    })
}
