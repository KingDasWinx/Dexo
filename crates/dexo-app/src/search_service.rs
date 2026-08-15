use dexo_driver_api::{CatalogObject, ObjectKind};

#[derive(Clone, Debug, PartialEq)]
pub struct SearchDocument {
    pub object: CatalogObject,
    pub recency: u64,
    pub favorite: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub object: CatalogObject,
    pub rank: u8,
}

pub struct SearchService {
    documents: Vec<Indexed>,
}

struct Indexed {
    document: SearchDocument,
    name: String,
    schema: String,
    kind: String,
    qualified: String,
}

impl SearchService {
    pub fn from_objects(objects: Vec<CatalogObject>) -> Self {
        Self::new(
            objects
                .into_iter()
                .map(|object| SearchDocument {
                    object,
                    recency: 0,
                    favorite: false,
                })
                .collect(),
        )
    }

    pub fn new(documents: Vec<SearchDocument>) -> Self {
        let documents = documents
            .into_iter()
            .map(|document| {
                let name = document.object.qualified_name.object().to_ascii_lowercase();
                let schema = document
                    .object
                    .qualified_name
                    .schema()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let kind = document.object.kind.as_str().to_ascii_lowercase();
                let qualified = document
                    .object
                    .qualified_name
                    .display_unquoted()
                    .to_ascii_lowercase();
                Indexed {
                    document,
                    name,
                    schema,
                    kind,
                    qualified,
                }
            })
            .collect();
        Self { documents }
    }

    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u8, bool, u64, String, SearchHit)> = self
            .documents
            .iter()
            .filter_map(|indexed| {
                let rank = best_rank(&query, indexed)?;
                Some((
                    rank,
                    indexed.document.favorite,
                    indexed.document.recency,
                    indexed.qualified.clone(),
                    SearchHit {
                        object: indexed.document.object.clone(),
                        rank,
                    },
                ))
            })
            .collect();
        hits.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.3.cmp(&b.3))
        });
        hits.into_iter().map(|item| item.4).collect()
    }
}

fn best_rank(query: &str, indexed: &Indexed) -> Option<u8> {
    [
        indexed.name.as_str(),
        indexed.schema.as_str(),
        indexed.kind.as_str(),
    ]
    .into_iter()
    .filter_map(|field| rank_field(query, field))
    .min()
}

fn rank_field(query: &str, field: &str) -> Option<u8> {
    if field == query {
        return Some(0);
    }
    if field.starts_with(query) {
        return Some(1);
    }
    if field
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(query))
    {
        return Some(2);
    }
    if is_subsequence(field, query) {
        return Some(3);
    }
    None
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|needed| chars.any(|ch| ch == needed))
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageHint {
    pub object_id: String,
    pub favorite: bool,
    pub recency: u64,
}

pub fn search_with_usage(
    objects: Vec<CatalogObject>,
    usage: &[UsageHint],
    restrictions: &[dexo_driver_api::CatalogRestriction],
    query: &str,
) -> Vec<SearchHit> {
    let documents = objects
        .into_iter()
        .filter(|object| {
            let name = object.qualified_name.object();
            let qualified = object.qualified_name.display_unquoted();
            !restrictions.iter().any(|restriction| {
                restriction.capability == name
                    || restriction.capability == qualified
                    || restriction.capability == object.id.as_str()
            })
        })
        .map(|object| {
            let hint = usage
                .iter()
                .find(|hint| hint.object_id == object.id.as_str());
            SearchDocument {
                favorite: hint.map(|hint| hint.favorite).unwrap_or(false),
                recency: hint.map(|hint| hint.recency).unwrap_or(0),
                object,
            }
        })
        .collect();
    SearchService::new(documents).search(query)
}

pub fn generate_catalog(count: usize) -> Vec<CatalogObject> {
    (0..count)
        .map(|index| {
            CatalogObject::new(
                dexo_driver_api::ObjectId::new(format!("obj:{index}")),
                if index % 17 == 0 {
                    ObjectKind::View
                } else {
                    ObjectKind::Table
                },
                dexo_driver_api::QualifiedName::new(
                    Some("db"),
                    Some(if index % 2 == 0 { "public" } else { "sales" }),
                    format!("obj_{index:06}"),
                ),
                None,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{SearchDocument, SearchService, generate_catalog};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use std::time::Instant;

    fn object(name: &str, schema: &str) -> CatalogObject {
        CatalogObject::new(
            ObjectId::new(name),
            ObjectKind::Table,
            QualifiedName::new(Some("db"), Some(schema), name),
            None,
        )
    }

    #[test]
    fn ranks_exact_prefix_word_start_subsequence() {
        let service = SearchService::new(vec![
            SearchDocument {
                object: object("catalog", "public"),
                recency: 0,
                favorite: false,
            },
            SearchDocument {
                object: object("cat", "public"),
                recency: 0,
                favorite: false,
            },
            SearchDocument {
                object: object("my_cat_table", "public"),
                recency: 0,
                favorite: false,
            },
            SearchDocument {
                object: object("create_at", "public"),
                recency: 9,
                favorite: true,
            },
        ]);
        let hits = service.search("cat");
        let names: Vec<_> = hits
            .iter()
            .map(|hit| hit.object.qualified_name.object().to_string())
            .collect();
        assert_eq!(names, vec!["cat", "catalog", "my_cat_table", "create_at"]);
        assert!(hits.windows(2).all(|pair| pair[0].rank <= pair[1].rank));
    }

    #[test]
    fn tie_break_is_qualified_name() {
        let service =
            SearchService::from_objects(vec![object("users", "sales"), object("users", "public")]);
        let hits = service.search("users");
        assert_eq!(
            hits[0].object.qualified_name.display_unquoted(),
            "db.public.users"
        );
        assert_eq!(
            hits[1].object.qualified_name.display_unquoted(),
            "db.sales.users"
        );
    }

    #[test]
    fn search_100k_objects_records_timing_without_gating() {
        let mut objects = generate_catalog(100_000);
        objects.push(object("needle", "public"));
        let service = SearchService::from_objects(objects);
        let started = Instant::now();
        let hits = service.search("needle");
        let elapsed_ms = started.elapsed().as_millis();
        assert_eq!(hits[0].object.qualified_name.object(), "needle");
        assert_eq!(hits[0].rank, 0);
        eprintln!("catalog_search_100k_ms={elapsed_ms}");
    }

    #[test]
    fn search_ranks_favorite_then_recent_without_returning_denied_objects() {
        let hits = super::search_with_usage(
            vec![
                object("orders", "public"),
                object("order_items", "public"),
                object("secret_orders", "private"),
            ],
            &[
                super::UsageHint {
                    object_id: "orders".into(),
                    favorite: true,
                    recency: 1,
                },
                super::UsageHint {
                    object_id: "order_items".into(),
                    favorite: false,
                    recency: 9,
                },
            ],
            &[dexo_driver_api::CatalogRestriction {
                parent: None,
                capability: "secret_orders".into(),
                reason: "permission denied".into(),
            }],
            "ord",
        );
        assert_eq!(hits[0].object.qualified_name.object(), "orders");
        assert!(
            hits.iter()
                .all(|hit| hit.object.qualified_name.object() != "secret_orders")
        );
    }
}
