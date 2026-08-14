use crate::theme::Role;

pub fn marker(role: Role, unicode: bool) -> &'static str {
    match (role, unicode) {
        (Role::Production, true) => "●PROD",
        (Role::Production, false) => "[PROD]",
        (Role::Staging, true) => "●STG",
        (Role::Staging, false) => "[STG]",
        (Role::Development, true) => "○DEV",
        (Role::Development, false) => "[DEV]",
        (Role::Error, true) => "✗",
        (Role::Error, false) => "[ERR]",
        (Role::Warning, true) => "!",
        (Role::Warning, false) => "[WARN]",
        (Role::Success, true) => "✓",
        (Role::Success, false) => "[OK]",
        (Role::Selection, true) => "▸",
        (Role::Selection, false) => ">",
        (Role::Focus, true) => "◆",
        (Role::Focus, false) => "*",
        _ => "",
    }
}

pub fn environment_marker(environment: &str, unicode: bool) -> &'static str {
    match environment.to_ascii_lowercase().as_str() {
        "production" => marker(Role::Production, unicode),
        "staging" => marker(Role::Staging, unicode),
        "development" => marker(Role::Development, unicode),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{environment_marker, marker};
    use crate::theme::Role;

    #[test]
    fn production_error_selection_differ_without_unicode() {
        let prod = marker(Role::Production, false);
        let err = marker(Role::Error, false);
        let sel = marker(Role::Selection, false);
        assert_ne!(prod, err);
        assert_ne!(prod, sel);
        assert_ne!(err, sel);
        assert!(prod.contains("PROD"));
        assert!(err.contains("ERR"));
        assert_eq!(sel, ">");
    }

    #[test]
    fn environment_marker_uses_text_when_ascii() {
        assert_eq!(environment_marker("production", false), "[PROD]");
        assert_eq!(environment_marker("local", false), "");
    }
}
