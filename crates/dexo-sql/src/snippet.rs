#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snippet {
    pub name: String,
    pub body: String,
}

pub fn expand_placeholders(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        let Some(end) = rest.find('}') else {
            out.push_str("${");
            out.push_str(rest);
            return out;
        };
        let inner = &rest[..end];
        rest = &rest[end + 1..];
        let default = inner.split_once(':').map(|(_, value)| value).unwrap_or("");
        out.push_str(default);
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::expand_placeholders;

    #[test]
    fn expands_tabstop_defaults() {
        assert_eq!(
            expand_placeholders("select ${1:name} from ${2:t}"),
            "select name from t"
        );
    }
}
