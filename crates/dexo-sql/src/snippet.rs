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

#[cfg(test)]
mod tests {
    use super::{expand, expand_placeholders};

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
