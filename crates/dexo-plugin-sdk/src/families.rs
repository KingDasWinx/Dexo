//! The families beyond queries.

use dexo_driver_api::{Capability, Session};
use dexo_plugin_protocol::{Family, RpcError, code};
use serde_json::Value;

use crate::Server;

pub(crate) fn of(session: &dyn Session) -> Vec<Family> {
    let mut families = Vec::new();
    if session
        .capabilities()
        .iter()
        .any(|state| state.capability == Capability::Cancel && state.available)
    {
        families.push(Family::Cancel);
    }
    families
}

pub(crate) async fn dispatch(_: &Server, method: &str, _: Value) -> Result<Value, RpcError> {
    Err(RpcError::new(
        code::METHOD_NOT_FOUND,
        format!("no method '{method}'"),
    ))
}
