#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snippet {
    pub name: String,
    pub body: String,
}

/// A snippet body with its placeholders filled in, and where they ended up.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Expansion {
    pub text: String,
    /// The editable holes, in the order Tab should walk them. Counted in characters,
    /// because that is what a cursor is counted in.
    pub stops: Vec<std::ops::Range<usize>>,
}

/// Expands `${1:default}` placeholders and records each one, so the editor can walk
/// them. The structure used to be thrown away the moment it was parsed: the default
/// text was substituted and a snippet became ordinary text with no holes to fill.
pub fn expand(body: &str) -> Expansion {
    let mut text = String::new();
    let mut chars = 0usize;
    let mut found: Vec<(u32, usize, std::ops::Range<usize>)> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("${") {
        text.push_str(&rest[..start]);
        chars += rest[..start].chars().count();
        rest = &rest[start + 2..];
        let Some(end) = rest.find('}') else {
            // Unterminated: it was not a placeholder after all.
            text.push_str("${");
            text.push_str(rest);
            return Expansion {
                text,
                stops: Vec::new(),
            };
        };
        let inner = &rest[..end];
        rest = &rest[end + 1..];
        let (order, default) = match inner.split_once(':') {
            Some((order, default)) => (order.parse::<u32>().ok(), default),
            None => (inner.parse::<u32>().ok(), ""),
        };
        let width = default.chars().count();
        // An explicit number orders the holes; the rest follow the order they appear in.
        found.push((order.unwrap_or(u32::MAX), found.len(), chars..chars + width));
        text.push_str(default);
        chars += width;
    }
    text.push_str(rest);
    found.sort_by_key(|(order, position, _)| (*order, *position));
    Expansion {
        text,
        stops: found.into_iter().map(|(_, _, range)| range).collect(),
    }
}

pub fn expand_placeholders(body: &str) -> String {
    expand(body).text
}

/// The snippets every install has. Nothing in the program makes a snippet, so without
/// these Insert Snippet only ever said there were none.
pub fn builtin_snippets() -> Vec<Snippet> {
    [
        ("select", "SELECT ${1:*}\nFROM ${2:table}\nWHERE ${3:condition};"),
        ("insert", "INSERT INTO ${1:table} (${2:columns})\nVALUES (${3:values});"),
        ("update", "UPDATE ${1:table}\nSET ${2:column} = ${3:value}\nWHERE ${4:condition};"),
        ("delete", "DELETE FROM ${1:table}\nWHERE ${2:condition};"),
        ("count", "SELECT count(*)\nFROM ${1:table}\nWHERE ${2:condition};"),
        ("join", "SELECT ${1:*}\nFROM ${2:a}\nJOIN ${3:b} ON ${3:b}.${4:a_id} = ${2:a}.id;"),
        ("cte", "WITH ${1:name} AS (\n  SELECT ${2:*}\n  FROM ${3:table}\n)\nSELECT *\nFROM ${1:name};"),
        ("group by", "SELECT ${1:column}, count(*)\nFROM ${2:table}\nGROUP BY ${1:column}\nORDER BY count(*) DESC;"),
    ]
    .into_iter()
    .map(|(name, body)| Snippet {
        name: name.into(),
        body: body.into(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{builtin_snippets, expand, expand_placeholders};

    #[test]
    fn the_builtin_snippets_expand_and_have_holes_to_fill() {
        let all = builtin_snippets();
        assert!(all.iter().any(|snippet| snippet.name == "select"));
        for snippet in all {
            let expansion = expand(&snippet.body);
            assert!(!expansion.stops.is_empty(), "{}", snippet.name);
            assert!(!expansion.text.contains("${"), "{}", snippet.name);
        }
    }

    #[test]
    fn expands_tabstop_defaults() {
        assert_eq!(
            expand_placeholders("select ${1:name} from ${2:t}"),
            "select name from t"
        );
    }

    #[test]
    fn records_where_the_holes_ended_up() {
        let expansion = expand("select ${1:name} from ${2:t}");
        assert_eq!(expansion.text, "select name from t");
        assert_eq!(expansion.stops, [7..11, 17..18]);
    }
}
