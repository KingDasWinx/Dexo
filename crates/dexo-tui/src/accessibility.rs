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

/// Read the way the editor's guard reads it: a label Dexo does not know, such as
/// `prod`, is production, so the marker never says less than the prompts do.
pub fn environment_marker(environment: &str, unicode: bool) -> &'static str {
    match environment_role(environment) {
        Some(role) => marker(role, unicode),
        None => "",
    }
}

pub fn environment_role(environment: &str) -> Option<Role> {
    match dexo_app::Environment::parse_strict(environment) {
        dexo_app::Environment::Production => Some(Role::Production),
        dexo_app::Environment::Staging => Some(Role::Staging),
        dexo_app::Environment::Development => Some(Role::Development),
        dexo_app::Environment::Local => None,
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

    /// A label Dexo does not know, such as `prod`, gets production's rules in the
    /// editor, so it gets production's marker too.
    #[test]
    fn unknown_labels_are_marked_as_production() {
        assert_eq!(environment_marker("prod", false), "[PROD]");
        assert_eq!(environment_marker("local", false), "");
        assert_eq!(environment_marker("", false), "");
    }
}
