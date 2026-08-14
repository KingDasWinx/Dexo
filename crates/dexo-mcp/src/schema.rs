use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SearchInput {
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ObjectInput {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct QueryInput {
    pub sql: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ExplainInput {
    pub sql: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct DiffInput {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TextOut {
    pub text: String,
}
