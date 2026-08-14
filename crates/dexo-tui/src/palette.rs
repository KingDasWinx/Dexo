use crate::action::{Action, FocusTarget};
use crate::model::Model;
use dexo_driver_api::TransactionState;

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub disabled_reason: Option<&'static str>,
    pub action: fn() -> Action,
}

pub fn palette_entries(model: &Model) -> Vec<PaletteEntry> {
    vec![
        PaletteEntry {
            id: "workbench.quit",
            title: "Quit",
            keywords: &["exit", "close"],
            shortcut: Some("Ctrl+Q"),
            disabled_reason: None,
            action: || Action::Quit,
        },
        PaletteEntry {
            id: "palette.open",
            title: "Command Palette",
            keywords: &["commands", "search"],
            shortcut: Some("Ctrl+P"),
            disabled_reason: None,
            action: || Action::OpenPalette,
        },
        PaletteEntry {
            id: "query.execute",
            title: "Execute Query",
            keywords: &["run", "sql"],
            shortcut: Some("F5"),
            disabled_reason: if model.sql.trim().is_empty() {
                Some("editor is empty")
            } else {
                None
            },
            action: || Action::ExecuteQuery,
        },
        PaletteEntry {
            id: "query.cancel",
            title: "Cancel Query",
            keywords: &["stop", "abort"],
            shortcut: Some("Ctrl+C"),
            disabled_reason: if model.active_query.is_none() {
                Some("no running query")
            } else {
                None
            },
            action: || Action::CancelQuery,
        },
        PaletteEntry {
            id: "transaction.commit",
            title: "Commit Transaction",
            keywords: &["tx", "commit"],
            shortcut: None,
            disabled_reason: if model.transaction == TransactionState::Active {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::CommitTransaction,
        },
        PaletteEntry {
            id: "transaction.rollback",
            title: "Rollback Transaction",
            keywords: &["tx", "abort"],
            shortcut: None,
            disabled_reason: if matches!(
                model.transaction,
                TransactionState::Active | TransactionState::Failed
            ) {
                None
            } else {
                Some("no active transaction")
            },
            action: || Action::RollbackTransaction,
        },
        PaletteEntry {
            id: "focus.explorer",
            title: "Focus Explorer",
            keywords: &["sidebar", "tree"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Explorer),
        },
        PaletteEntry {
            id: "focus.editor",
            title: "Focus Editor",
            keywords: &["sql", "query"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Editor),
        },
        PaletteEntry {
            id: "focus.results",
            title: "Focus Results",
            keywords: &["grid", "rows"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Results),
        },
        PaletteEntry {
            id: "focus.inspector",
            title: "Focus Inspector",
            keywords: &["details", "side"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::Focus(FocusTarget::Inspector),
        },
        PaletteEntry {
            id: "data.copy.csv",
            title: "Copy as CSV",
            keywords: &["clipboard", "grid"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::CopyGrid(dexo_app::data::CopyFormat::Csv),
        },
        PaletteEntry {
            id: "data.review",
            title: "Review Changes",
            keywords: &["apply", "edit"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenReview,
        },
        PaletteEntry {
            id: "data.related",
            title: "Open Related",
            keywords: &["foreign", "key"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRelated,
        },
        PaletteEntry {
            id: "data.inspect",
            title: "Inspect Value",
            keywords: &["viewer", "json"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::InspectValue,
        },
        PaletteEntry {
            id: "schema.preview",
            title: "Preview DDL",
            keywords: &["schema", "ddl", "form"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDdlPreview,
        },
        PaletteEntry {
            id: "schema.raw",
            title: "Apply Raw DDL",
            keywords: &["sql", "escape"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ApplyRawDdl,
        },
        PaletteEntry {
            id: "schema.diff",
            title: "Compare Schema",
            keywords: &["diff", "migration", "snapshot"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSchemaDiff,
        },
        PaletteEntry {
            id: "transfer.export",
            title: "Export Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenTransfer,
        },
        PaletteEntry {
            id: "transfer.import",
            title: "Import Data",
            keywords: &["csv", "json", "file"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenTransfer,
        },
        PaletteEntry {
            id: "backup.dump",
            title: "Native Backup",
            keywords: &["pg_dump", "mysqldump"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenBackup,
        },
        PaletteEntry {
            id: "backup.restore",
            title: "Native Restore",
            keywords: &["pg_restore", "mysql"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRestore,
        },
        PaletteEntry {
            id: "schema.security",
            title: "Manage Grants",
            keywords: &["role", "user", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSecurity,
        },
        PaletteEntry {
            id: "explain.open",
            title: "Explain Plan",
            keywords: &["analyze", "plan", "cost"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenExplain,
        },
        PaletteEntry {
            id: "admin.sessions",
            title: "Inspect Sessions",
            keywords: &["locks", "cancel", "terminate"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenAdmin,
        },
        PaletteEntry {
            id: "mcp.profiles",
            title: "MCP Profiles",
            keywords: &["mcp", "allowlist", "policy", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenMcpProfiles,
        },
        PaletteEntry {
            id: "explorer.expand",
            title: "Expand Explorer Node",
            keywords: &["tree", "open"],
            shortcut: Some("Enter"),
            disabled_reason: None,
            action: || Action::ExplorerExpand,
        },
        PaletteEntry {
            id: "explorer.copy_name",
            title: "Copy Object Name",
            keywords: &["clipboard", "tree"],
            shortcut: Some("c"),
            disabled_reason: None,
            action: || Action::ExplorerCopyName,
        },
        PaletteEntry {
            id: "results.up",
            title: "Results Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("Up"),
            disabled_reason: None,
            action: || Action::ResultsUp,
        },
        PaletteEntry {
            id: "results.down",
            title: "Results Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("Down"),
            disabled_reason: None,
            action: || Action::ResultsDown,
        },
        PaletteEntry {
            id: "results.left",
            title: "Results Left",
            keywords: &["grid", "scroll"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsLeft,
        },
        PaletteEntry {
            id: "results.right",
            title: "Results Right",
            keywords: &["grid", "scroll"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsRight,
        },
        PaletteEntry {
            id: "results.pageup",
            title: "Results Page Up",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageUp"),
            disabled_reason: None,
            action: || Action::ResultsPageUp,
        },
        PaletteEntry {
            id: "results.pagedown",
            title: "Results Page Down",
            keywords: &["grid", "scroll"],
            shortcut: Some("PageDown"),
            disabled_reason: None,
            action: || Action::ResultsPageDown,
        },
        PaletteEntry {
            id: "results.top",
            title: "Results Top",
            keywords: &["grid", "home"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ResultsTop,
        },
        PaletteEntry {
            id: "settings.open",
            title: "Open Settings",
            keywords: &["theme", "keymap", "mouse"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenSettings,
        },
        PaletteEntry {
            id: "settings.reset",
            title: "Reset Settings",
            keywords: &["defaults"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmResetSettings,
        },
        PaletteEntry {
            id: "recovery.open",
            title: "Session Recovery",
            keywords: &["crash", "restore"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenRecovery,
        },
        PaletteEntry {
            id: "recovery.restore",
            title: "Recover Session",
            keywords: &["crash"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmRecover,
        },
        PaletteEntry {
            id: "recovery.discard",
            title: "Discard Recovery",
            keywords: &["crash"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::ConfirmDiscardRecovery,
        },
        PaletteEntry {
            id: "mcp.audit",
            title: "MCP Audit Log",
            keywords: &["mcp", "grant", "revoke"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenMcpAudit,
        },
        PaletteEntry {
            id: "mcp.revoke_all",
            title: "Revoke All MCP Grants",
            keywords: &["mcp", "grant"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::RevokeAllMcpGrants,
        },
        PaletteEntry {
            id: "diagnostics.export",
            title: "Export Diagnostics",
            keywords: &["logs", "support"],
            shortcut: None,
            disabled_reason: None,
            action: || Action::OpenDiagnostics,
        },
    ]
}

pub fn action_by_id(id: &str) -> Option<Action> {
    palette_entries(&Model::default())
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| (entry.action)())
}

pub fn filter_entries<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    if query.is_empty() {
        return entries.iter().collect();
    }
    let mut scored: Vec<(u8, &PaletteEntry)> = entries
        .iter()
        .filter_map(|entry| score(entry, query).map(|s| (s, entry)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(b.1.title)));
    scored.into_iter().map(|(_, entry)| entry).collect()
}

fn score(entry: &PaletteEntry, query: &str) -> Option<u8> {
    let query = query.to_ascii_lowercase();
    let haystacks = std::iter::once(entry.title)
        .chain(entry.keywords.iter().copied())
        .chain(std::iter::once(entry.id));
    haystacks.filter_map(|text| score_text(text, &query)).max()
}

fn score_text(text: &str, query: &str) -> Option<u8> {
    let text = text.to_ascii_lowercase();
    if text.starts_with(query) {
        return Some(3);
    }
    if text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        return Some(2);
    }
    if is_subsequence(&text, query) {
        return Some(1);
    }
    None
}

fn is_subsequence(text: &str, query: &str) -> bool {
    let mut chars = text.chars();
    query.chars().all(|needle| chars.any(|ch| ch == needle))
}

#[cfg(test)]
mod tests {
    use super::{filter_entries, palette_entries};
    use crate::model::Model;
    use dexo_driver_api::TransactionState;

    #[test]
    fn palette_explains_disabled_commit() {
        let entries = palette_entries(&Model::fixture(TransactionState::Idle));
        let commit = entries
            .iter()
            .find(|e| e.id == "transaction.commit")
            .unwrap();
        assert_eq!(commit.disabled_reason, Some("no active transaction"));
    }

    #[test]
    fn fuzzy_prefers_prefix_over_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "quit");
        assert_eq!(filtered[0].id, "workbench.quit");
    }

    #[test]
    fn fuzzy_word_start_beats_subsequence() {
        let entries = palette_entries(&Model::default());
        let filtered = filter_entries(&entries, "pal");
        assert_eq!(filtered[0].id, "palette.open");
    }

    #[test]
    fn every_current_action_is_in_palette() {
        let ids: Vec<_> = palette_entries(&Model::default())
            .iter()
            .map(|entry| entry.id)
            .collect();
        for id in [
            "workbench.quit",
            "palette.open",
            "query.execute",
            "query.cancel",
            "transaction.commit",
            "transaction.rollback",
            "focus.explorer",
            "focus.editor",
            "focus.results",
            "focus.inspector",
        ] {
            assert!(ids.contains(&id), "missing {id}");
        }
    }
}
