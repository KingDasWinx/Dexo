use std::collections::HashMap;
use std::time::{Duration, Instant};

use dexo_app::mcp::McpService;
use rmcp::model::Resource;

use crate::error::hidden_error;

pub struct ResultStore {
    pages: HashMap<String, ResultPage>,
    profile: String,
}

struct ResultPage {
    body: String,
    expires: Instant,
    owner: String,
}

impl ResultStore {
    pub fn new(profile: impl Into<String>) -> Self {
        Self {
            pages: HashMap::new(),
            profile: profile.into(),
        }
    }

    pub fn insert(&mut self, uri: String, body: String, ttl: Duration) {
        self.pages.insert(
            uri,
            ResultPage {
                body,
                expires: Instant::now() + ttl,
                owner: self.profile.clone(),
            },
        );
    }

    pub fn get(&self, uri: &str) -> Result<&str, String> {
        match self.pages.get(uri) {
            Some(page) if page.expires > Instant::now() && page.owner == self.profile => {
                Ok(&page.body)
            }
            _ => Err(hidden_error().into()),
        }
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }
}

pub fn list_resources(service: &McpService, _store: &ResultStore) -> Vec<Resource> {
    let mut out = vec![resource(
        "dexo://profile/capabilities",
        "Active MCP capabilities",
    )];
    for object in service.search("") {
        let name = object.qualified_name.display_unquoted();
        out.push(resource(
            &format!("dexo://object/{}", object.id.as_str()),
            &name,
        ));
        out.push(resource(
            &format!("dexo://ddl/{}", object.id.as_str()),
            &format!("{name} ddl"),
        ));
        out.push(resource(
            &format!("dexo://deps/{}", object.id.as_str()),
            &format!("{name} deps"),
        ));
    }
    out
}

pub fn read_resource(
    service: &McpService,
    store: &ResultStore,
    uri: &str,
) -> Result<String, String> {
    if uri == "dexo://profile/capabilities" {
        return Ok(service.capabilities().to_string());
    }
    if let Some(id) = uri.strip_prefix("dexo://object/") {
        return service
            .describe(id)
            .map(|object| serde_json::to_string(&object).unwrap_or_else(|_| hidden_error().into()))
            .map_err(|error| error.to_string());
    }
    if let Some(id) = uri.strip_prefix("dexo://ddl/") {
        return service.ddl(id).map_err(|error| error.to_string());
    }
    if let Some(id) = uri.strip_prefix("dexo://deps/") {
        return service
            .relationships(id)
            .map(|items| serde_json::to_string(&items).unwrap_or_else(|_| hidden_error().into()))
            .map_err(|error| error.to_string());
    }
    if uri.starts_with("dexo://result/") {
        return store.get(uri).map(str::to_string);
    }
    Err(hidden_error().into())
}

fn resource(uri: &str, name: &str) -> Resource {
    Resource::new(uri, name)
}
