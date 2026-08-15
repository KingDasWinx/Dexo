use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_app::data::{inspect_value, related_filter};
use dexo_driver_api::{DbValue, QueryRequest, TransactionState};

use crate::action::{Action, Effect, FocusTarget};
use crate::model::{Focus, GridModel, Model};

pub fn update(model: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Key(key) => handle_key(model, key),
        Action::Mouse(_) if !model.mouse => Vec::new(),
        Action::Mouse(_) => Vec::new(),
        Action::Resize { width, height } => {
            model.apply_size(width, height);
            vec![Effect::PersistLayout]
        }
        Action::ConnectionChanged {
            name,
            ready,
            environment,
            session,
            generation,
        } => {
            model.connection.name = name;
            model.connection.ready = ready;
            model.connection.environment = environment;
            model.active_session = session;
            model.session_generation = generation;
            model.connection_form.close();
            Vec::new()
        }
        Action::OpenConnectionForm => {
            model.connection_form = crate::screens::connection::ConnectionForm::open();
            Vec::new()
        }
        Action::ConnectionFormError { message } => {
            model.connection_form.set_error(message);
            Vec::new()
        }
        Action::SaveConnection => save_connection(model),
        Action::QueryResultSetStarted { key, index } => {
            if operation_matches(model, &key) {
                while model.result_tabs.len() <= index {
                    model.result_tabs.push(GridModel::default());
                }
                model.result_tabs[index].clear();
                model.results = GridModel::default();
            }
            Vec::new()
        }
        Action::QueryMeta { key, columns } => {
            if operation_matches(model, &key) {
                model.results.set_columns(columns.clone());
                if let Some(tab) = model.result_tabs.last_mut() {
                    tab.set_columns(columns);
                }
            }
            Vec::new()
        }
        Action::QueryRows { key, rows } => {
            if operation_matches(model, &key) {
                model.results.append_rows(rows.clone());
                if let Some(tab) = model.result_tabs.last_mut() {
                    tab.append_rows(rows);
                }
            }
            Vec::new()
        }
        Action::QueryNotice { key, message } => {
            if operation_matches(model, &key) {
                model.messages.push(message);
            }
            Vec::new()
        }
        Action::QueryResultSetFinished { .. } => Vec::new(),
        Action::ScriptFinished { .. } => {
            model.active_task = None;
            model.active_query = None;
            model.active_operation = None;
            Vec::new()
        }
        Action::CheckpointTick => Vec::new(),
        Action::TransactionChanged {
            session,
            generation,
            state,
        } => {
            if model.active_session == Some(session) && model.session_generation == generation {
                model.transaction = state;
            }
            Vec::new()
        }
        Action::OperationStarted(key) => {
            model.active_operation = Some(key.operation);
            Vec::new()
        }
        Action::OperationFailed { message, .. } => {
            model.active_operation = None;
            model.active_query = None;
            model.messages.push(message);
            Vec::new()
        }
        Action::OperationCancelled(_) => {
            model.active_operation = None;
            model.active_query = None;
            Vec::new()
        }
        Action::Bootstrapped(state) => {
            apply_bootstrap(model, state);
            Vec::new()
        }
        Action::OpenPalette => {
            open_palette(model);
            Vec::new()
        }
        Action::ClosePalette => {
            close_palette(model);
            Vec::new()
        }
        Action::PaletteQuery(query) => {
            model.palette.query = query;
            model.palette.selected = 0;
            model.palette.offset = 0;
            Vec::new()
        }
        Action::PaletteSelect => palette_select(model),
        Action::ExecuteQuery => start_query(model),
        Action::CancelQuery => cancel_query(model),
        Action::BeginTransaction => {
            if model.transaction == TransactionState::Idle {
                if let Some(session) = model.active_session {
                    vec![Effect::BeginTransaction {
                        session,
                        mode: dexo_driver_api::TransactionMode::ReadWrite,
                    }]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        Action::Savepoint => {
            if model.transaction == TransactionState::Active {
                if let Some(session) = model.active_session {
                    vec![Effect::Savepoint {
                        session,
                        name: "sp1".into(),
                    }]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        Action::CommitTransaction => {
            if model.transaction == TransactionState::Active {
                if let Some(session) = model.active_session {
                    vec![Effect::CommitTransaction { session }]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        Action::RollbackTransaction => {
            if model.transaction == TransactionState::Active
                || model.transaction == TransactionState::Failed
            {
                if let Some(session) = model.active_session {
                    vec![Effect::RollbackTransaction { session }]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        Action::Focus(target) => {
            model.focus = match target {
                FocusTarget::Explorer => Focus::Explorer,
                FocusTarget::Editor => Focus::Editor,
                FocusTarget::Results => Focus::Results,
                FocusTarget::Inspector => Focus::Inspector,
            };
            close_palette(model);
            Vec::new()
        }
        Action::ExplorerExpand => {
            if let Some(id) = model.explorer.selected.clone() {
                let _ = model.explorer.expand(&id);
            }
            Vec::new()
        }
        Action::ExplorerCopyName => {
            model.explorer.copy_selected_name();
            Vec::new()
        }
        Action::CopyGrid(format) => {
            if let Ok(text) = model.results.copy(format, model.data.dialect) {
                model.data.clipboard = text;
            }
            Vec::new()
        }
        Action::OpenReview => {
            model.data.open_review();
            Vec::new()
        }
        Action::ConfirmProduction => {
            model.data.confirm_production();
            Vec::new()
        }
        Action::ApplyChanges => {
            model.data.apply();
            Vec::new()
        }
        Action::FailApply => {
            model.data.fail_apply();
            Vec::new()
        }
        Action::RevertChanges => {
            model.data.revert();
            Vec::new()
        }
        Action::InspectValue => {
            inspect_selected(model);
            Vec::new()
        }
        Action::OpenRelated => {
            open_related(model);
            Vec::new()
        }
        Action::OpenDdlPreview => {
            open_ddl_preview(model);
            Vec::new()
        }
        Action::ConfirmDdl => {
            model.schema_editor.confirm_typed();
            Vec::new()
        }
        Action::ApplyDdl => {
            apply_ddl(model);
            Vec::new()
        }
        Action::ApplyRawDdl => {
            if !model.sql.trim().is_empty() {
                model.schema_editor.apply_raw(model.sql.clone());
            }
            Vec::new()
        }
        Action::OpenSecurity => {
            model.security.open = true;
            Vec::new()
        }
        Action::SchemaFocusNext => {
            model.schema_editor.focus_next();
            Vec::new()
        }
        Action::OpenSchemaDiff => {
            model.schema_diff = crate::screens::schema_diff::SchemaDiffScreen::fixture();
            Vec::new()
        }
        Action::SchemaDiffToggleAdded => {
            model.schema_diff.toggle_added();
            Vec::new()
        }
        Action::SchemaDiffToggleRemoved => {
            model.schema_diff.toggle_removed();
            Vec::new()
        }
        Action::SchemaDiffToggleChanged => {
            model.schema_diff.toggle_changed();
            Vec::new()
        }
        Action::ConfirmSchemaDiff => {
            model.schema_diff.confirm();
            Vec::new()
        }
        Action::ApplySchemaDiff => {
            model.schema_diff.apply();
            Vec::new()
        }
        Action::OpenTransfer => {
            model.transfer = crate::screens::transfer::TransferScreen::fixture_preview();
            Vec::new()
        }
        Action::OpenBackup => {
            model.transfer = crate::screens::transfer::TransferScreen::fixture_progress();
            model.transfer.mode = "backup";
            Vec::new()
        }
        Action::OpenRestore => {
            model.transfer = crate::screens::transfer::TransferScreen::fixture_rejects();
            model.transfer.mode = "restore";
            Vec::new()
        }
        Action::OpenExplain => {
            model.tabs.active = 4;
            model.explain = crate::screens::explain::ExplainScreen::fixture();
            Vec::new()
        }
        Action::ExplainViewTree => {
            model.explain.view = crate::screens::explain::ExplainView::Tree;
            Vec::new()
        }
        Action::ExplainViewTable => {
            model.explain.view = crate::screens::explain::ExplainView::Table;
            Vec::new()
        }
        Action::ExplainViewSummary => {
            model.explain.view = crate::screens::explain::ExplainView::Summary;
            Vec::new()
        }
        Action::ConfirmExplainAnalyze => {
            model.explain.analyze_confirmed = true;
            Vec::new()
        }
        Action::OpenAdmin => {
            model.admin = crate::screens::admin::AdminScreen::fixture();
            Vec::new()
        }
        Action::AdminPause => {
            model.admin.pause();
            Vec::new()
        }
        Action::AdminResume => {
            model.admin.resume();
            Vec::new()
        }
        Action::ConfirmAdmin => {
            model.admin.confirmed = true;
            model.admin.confirm_target = model
                .admin
                .sessions
                .first()
                .map(|session| session.id.clone())
                .unwrap_or_default();
            Vec::new()
        }
        Action::OpenMcpProfiles => {
            model.mcp_profiles = crate::screens::mcp_profiles::McpProfilesScreen::fixture();
            Vec::new()
        }
        Action::ConfirmMcpEnable => {
            model.mcp_profiles.confirm_enable();
            Vec::new()
        }
        Action::RevokeAllMcpGrants => {
            model.mcp_profiles.revoke_all();
            model.mcp_audit.revoke_all();
            Vec::new()
        }
        Action::OpenSettings => {
            model.settings = crate::screens::settings::SettingsScreen::fixture();
            Vec::new()
        }
        Action::ConfirmResetSettings => {
            if !model.settings.confirm_reset {
                model.settings.confirm_reset = true;
            } else {
                model.settings.reset();
            }
            Vec::new()
        }
        Action::OpenRecovery => {
            model.recovery = crate::screens::recovery::RecoveryScreen::fixture();
            Vec::new()
        }
        Action::ConfirmRecover => {
            model.recovery.recover();
            Vec::new()
        }
        Action::ConfirmDiscardRecovery => {
            if !model.recovery.confirm_discard {
                model.recovery.confirm_discard = true;
            } else {
                model.recovery.discard();
            }
            Vec::new()
        }
        Action::OpenMcpAudit => {
            model.mcp_audit = crate::screens::mcp_audit::McpAuditScreen::fixture();
            Vec::new()
        }
        Action::OpenDiagnostics => {
            model
                .messages
                .push("diagnostic preview ready; never uploaded".into());
            Vec::new()
        }
        Action::ResultsUp => {
            model.results.scroll_rows(-1);
            Vec::new()
        }
        Action::ResultsDown => {
            model.results.scroll_rows(1);
            Vec::new()
        }
        Action::ResultsLeft => {
            model.results.scroll_columns(-1);
            Vec::new()
        }
        Action::ResultsRight => {
            model.results.scroll_columns(1);
            Vec::new()
        }
        Action::ResultsPageUp => {
            model
                .results
                .scroll_rows(-(model.results.viewport().height as i32));
            Vec::new()
        }
        Action::ResultsPageDown => {
            model
                .results
                .scroll_rows(model.results.viewport().height as i32);
            Vec::new()
        }
        Action::ResultsTop => {
            let offset = model.results.viewport().row_offset as i32;
            model.results.scroll_rows(-offset);
            Vec::new()
        }
        Action::Quit => vec![Effect::Quit],
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if key.kind != KeyEventKind::Press {
        return Vec::new();
    }
    if model.palette.open {
        return handle_palette_key(model, key);
    }
    if model.connection_form.open {
        return handle_connection_form_key(model, key);
    }
    if model.schema_editor.preview.is_some() {
        return match key.code {
            KeyCode::Esc => {
                model.schema_editor.preview = None;
                Vec::new()
            }
            KeyCode::Enter => {
                apply_ddl(model);
                Vec::new()
            }
            KeyCode::Char(ch) => {
                if let Some(preview) = &mut model.schema_editor.preview {
                    preview.typed.push(ch);
                    model.schema_editor.confirm_typed();
                }
                Vec::new()
            }
            KeyCode::Backspace => {
                if let Some(preview) = &mut model.schema_editor.preview {
                    preview.typed.pop();
                    model.schema_editor.confirm_typed();
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.schema_diff.open {
        return match key.code {
            KeyCode::Esc => {
                model.schema_diff.open = false;
                Vec::new()
            }
            KeyCode::Char('a') => {
                model.schema_diff.toggle_added();
                Vec::new()
            }
            KeyCode::Char('r') => {
                model.schema_diff.toggle_removed();
                Vec::new()
            }
            KeyCode::Char('c') => {
                model.schema_diff.toggle_changed();
                Vec::new()
            }
            KeyCode::Char('y') => {
                model.schema_diff.confirm();
                Vec::new()
            }
            KeyCode::Enter => {
                model.schema_diff.apply();
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.transfer.open {
        return match key.code {
            KeyCode::Esc => {
                model.transfer.open = false;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.data.review.is_some() {
        return match key.code {
            KeyCode::Esc => {
                model.data.review = None;
                Vec::new()
            }
            KeyCode::Enter => {
                model.data.apply();
                Vec::new()
            }
            KeyCode::Char('y') => {
                model.data.confirm_production();
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.mcp_profiles.open {
        return match key.code {
            KeyCode::Esc => {
                model.mcp_profiles.open = false;
                Vec::new()
            }
            KeyCode::Char('r') => {
                model.mcp_profiles.revoke_all();
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.settings.open {
        return match key.code {
            KeyCode::Esc => {
                model.settings.open = false;
                Vec::new()
            }
            KeyCode::Char('r') => update(model, Action::ConfirmResetSettings),
            _ => Vec::new(),
        };
    }
    if model.recovery.open {
        return match key.code {
            KeyCode::Esc => {
                model.recovery.open = false;
                Vec::new()
            }
            KeyCode::Char('y') => update(model, Action::ConfirmRecover),
            KeyCode::Char('n') => update(model, Action::ConfirmDiscardRecovery),
            _ => Vec::new(),
        };
    }
    if model.mcp_audit.open {
        return match key.code {
            KeyCode::Esc => {
                model.mcp_audit.open = false;
                Vec::new()
            }
            KeyCode::Char('r') => update(model, Action::RevokeAllMcpGrants),
            _ => Vec::new(),
        };
    }
    let spec = crate::keymap::KeySpec {
        modifiers: key.modifiers,
        code: key.code,
    };
    let mut chord = model.pending_chord.clone();
    chord.keys.push(spec);
    let ctx = active_key_context(model);
    if model.keymap.is_prefix(&chord, ctx) {
        model.pending_chord = chord;
        return Vec::new();
    }
    match model.keymap.resolve(&chord, ctx) {
        Ok(Some(command)) => {
            model.pending_chord.keys.clear();
            if let Some(action) = crate::palette::action_by_id(command) {
                return update(model, action);
            }
        }
        Ok(None) => model.pending_chord.keys.clear(),
        Err(conflict) => {
            model.pending_chord.keys.clear();
            model.messages.push(format!(
                "keymap conflict {}: {}",
                conflict.chord,
                conflict.commands.join(" / ")
            ));
            return Vec::new();
        }
    }
    if key.code == KeyCode::Tab && model.tabs.active == 2 {
        model.schema_editor.focus_next();
    }
    Vec::new()
}

fn active_key_context(model: &Model) -> crate::keymap::KeyContext {
    use crate::keymap::KeyContext;
    if model.palette.open {
        return KeyContext::Palette;
    }
    match model.focus {
        Focus::Explorer => KeyContext::Explorer,
        Focus::Results => KeyContext::Results,
        Focus::Inspector => KeyContext::Inspector,
        Focus::Editor | Focus::Palette => KeyContext::Editor,
    }
}

fn handle_palette_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            close_palette(model);
            Vec::new()
        }
        KeyCode::Enter => palette_select(model),
        KeyCode::Up => {
            move_palette_selection(model, -1);
            Vec::new()
        }
        KeyCode::Down => {
            move_palette_selection(model, 1);
            Vec::new()
        }
        KeyCode::Backspace => {
            model.palette.query.pop();
            model.palette.selected = 0;
            model.palette.offset = 0;
            Vec::new()
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            model.palette.query.push(ch);
            model.palette.selected = 0;
            model.palette.offset = 0;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_connection_form_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.connection_form.close();
            Vec::new()
        }
        KeyCode::Enter => save_connection(model),
        KeyCode::Tab | KeyCode::Down => {
            model.connection_form.focus_next();
            Vec::new()
        }
        KeyCode::BackTab | KeyCode::Up => {
            model.connection_form.focus_prev();
            Vec::new()
        }
        KeyCode::Backspace => {
            model.connection_form.backspace();
            Vec::new()
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            model.connection_form.type_char(ch);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn save_connection(model: &mut Model) -> Vec<Effect> {
    match model.connection_form.submit() {
        Some((input, password)) => vec![Effect::CreateConnection { input, password }],
        None => Vec::new(),
    }
}

fn open_palette(model: &mut Model) {
    model.palette.open = true;
    model.palette.query.clear();
    model.palette.selected = 0;
    model.palette.offset = 0;
    model.focus = Focus::Palette;
}

fn move_palette_selection(model: &mut Model, delta: isize) {
    let count = crate::palette::filter_entries(
        &crate::palette::palette_entries(model),
        &model.palette.query,
    )
    .len();
    if count == 0 {
        model.palette.selected = 0;
        model.palette.offset = 0;
        return;
    }
    let selected = (model.palette.selected as isize + delta).clamp(0, count as isize - 1) as usize;
    model.palette.selected = selected;
    model.palette.offset = crate::palette::scroll_to_selection(
        selected,
        model.palette.offset,
        count,
        crate::palette::popup_list_rows(model.height),
    );
}

fn close_palette(model: &mut Model) {
    if model.palette.open {
        model.palette.open = false;
        if model.focus == Focus::Palette {
            model.focus = Focus::Editor;
        }
    }
}

fn start_query(model: &mut Model) -> Vec<Effect> {
    if model.sql.trim().is_empty() {
        return Vec::new();
    }
    let statements = crate::screens::workbench::planned_statements(model);
    if statements.is_empty() {
        return Vec::new();
    }
    model.results.clear();
    model.result_tabs = statements.iter().map(|_| GridModel::default()).collect();
    let request = QueryRequest::read(statements[0].clone(), 10_000);
    model.active_query = Some(request.id);
    let operation = crate::runtime::OperationId::new();
    model.active_operation = Some(operation);
    let session = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    vec![Effect::StartScript(crate::action::ScriptRequest {
        key: crate::runtime::OperationKey::new(
            operation,
            session,
            "scratch",
            model.session_generation.max(1),
        ),
        statements,
        policy: model.script_policy,
        parameters: Vec::new(),
        timeout: std::time::Duration::from_secs(30),
    })]
}

fn cancel_query(model: &mut Model) -> Vec<Effect> {
    model
        .active_operation
        .map(Effect::CancelOperation)
        .into_iter()
        .collect()
}

fn apply_bootstrap(model: &mut Model, state: crate::runtime::storage_worker::BootstrapState) {
    model.project = state.active_project.name;
    model.project_id = state.active_project.id.0.to_string();
    if let Some(layout) = state.layout {
        model.panes.explorer_visible = layout.explorer_visible;
        model.panes.inspector_visible = layout.inspector_visible;
        model.panes.results_visible = layout.results_visible;
        model.panes.explorer_width = layout.explorer_width;
        model.panes.inspector_width = layout.inspector_width;
        model.panes.results_height = layout.results_height;
        model.tabs.active = layout.active_tab;
        if !layout.tabs.is_empty() {
            model.tabs.titles = layout.tabs;
        }
    }
    if state.recovery.needs_recovery() {
        model.recovery.open = true;
        model.recovery.transaction = state.recovery.transaction;
        model.recovery.documents = state
            .recovery
            .documents
            .into_iter()
            .map(|document| document.title)
            .collect();
    }
}

fn operation_matches(model: &Model, key: &crate::runtime::OperationKey) -> bool {
    let session = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    let generation = if model.session_generation == 0 {
        key.generation
    } else {
        model.session_generation
    };
    let document = "scratch";
    key.belongs_to(&session, document, generation)
}

fn inspect_selected(model: &mut Model) {
    let Some((row, col)) = model.results.selection() else {
        return;
    };
    let Some(value) = model
        .results
        .rows()
        .get(row)
        .and_then(|cells| cells.get(col))
    else {
        return;
    };
    let loaded = match value {
        DbValue::Bytes(bytes) | DbValue::Native { bytes, .. } => bytes.len() as u64,
        DbValue::Text(text) | DbValue::Json(text) => text.len() as u64,
        _ => 0,
    };
    model.data.viewer = Some(inspect_value(value, loaded, loaded));
}

fn open_related(model: &mut Model) {
    let Some(fk) = model.data.related_fk.clone() else {
        return;
    };
    if related_filter(&fk, &model.data.related_row).is_none() {
        return;
    }
    let title = fk.referenced_table.display_unquoted();
    model.tabs.titles.push(title.clone());
    model.tabs.active = model.tabs.titles.len() - 1;
    model.result_tabs.push(GridModel::default());
    model.data.related_open.push(title);
}

fn open_ddl_preview(model: &mut Model) {
    if !model.schema_editor.validate() {
        return;
    }
    let Ok(change) = model.schema_editor.to_change() else {
        return;
    };
    let sql = format!(
        "{} {}",
        match &change {
            dexo_driver_api::SchemaChange::CreateTable { .. } => "CREATE TABLE",
            dexo_driver_api::SchemaChange::AlterTable { .. } => "ALTER TABLE",
            dexo_driver_api::SchemaChange::CreateView { .. } => "CREATE VIEW",
            dexo_driver_api::SchemaChange::AlterRoutine { .. } => "ALTER ROUTINE",
            dexo_driver_api::SchemaChange::CreateIndex { .. } => "CREATE INDEX",
            dexo_driver_api::SchemaChange::DropObject { .. } => "DROP",
            dexo_driver_api::SchemaChange::RenameObject { .. } => "RENAME",
            dexo_driver_api::SchemaChange::Grant { .. } => "GRANT",
            dexo_driver_api::SchemaChange::Revoke { .. } => "REVOKE",
        },
        change.target().display_unquoted()
    );
    let plan = dexo_driver_api::DdlPlan {
        statements: vec![dexo_driver_api::DdlStatement {
            sql,
            implicit_commit: false,
        }],
        rollback: vec![],
        warnings: vec![],
        transactional: true,
    };
    let preview = dexo_app::schema::preview_change(
        &change,
        plan,
        Vec::new(),
        Vec::new(),
        &dexo_app::schema::production_policy(),
    );
    model.schema_editor.open_preview(preview);
}

fn apply_ddl(model: &mut Model) {
    let Some(preview) = &model.schema_editor.preview else {
        return;
    };
    if matches!(
        preview.confirmation,
        dexo_app::schema::Confirmation::TypeTarget(_)
    ) && !preview.confirmed
    {
        return;
    }
    model.schema_editor.preview = None;
    model.messages.push("ddl queued".into());
}

fn palette_select(model: &mut Model) -> Vec<Effect> {
    let entries = crate::palette::palette_entries(model);
    let visible = crate::palette::filter_entries(&entries, &model.palette.query);
    let Some(entry) = visible.get(model.palette.selected) else {
        return Vec::new();
    };
    if entry.disabled_reason.is_some() {
        return Vec::new();
    }
    let action = (entry.action)();
    close_palette(model);
    update(model, action)
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::DbValue;

    use super::update;
    use crate::action::{Action, Effect};
    use crate::model::{Focus, Model};
    use crate::runtime::{OperationId, OperationKey};

    #[test]
    fn query_events_do_not_change_editor_focus() {
        let mut model = Model::fixture(Focus::Editor);
        let key = OperationKey::new(OperationId::new(), "", "scratch", 1);
        update(
            &mut model,
            Action::QueryRows {
                key,
                rows: vec![vec![DbValue::I64(1)]],
            },
        );
        assert_eq!(model.focus, Focus::Editor);
        assert_eq!(model.results.row_count(), 1);
    }

    #[test]
    fn save_connection_clears_password_and_emits_create() {
        let mut model = Model::default();
        update(&mut model, Action::OpenConnectionForm);
        for (label, value) in [
            ("name", "local-pg"),
            ("driver", "postgres"),
            ("host", "127.0.0.1"),
            ("database", "dexo"),
            ("username", "dexo"),
            ("password", "SUPER_SECRET_SENTINEL"),
        ] {
            let field = model
                .connection_form
                .fields
                .iter_mut()
                .find(|field| field.label == label)
                .unwrap();
            field.value = value.into();
        }
        let effects = update(&mut model, Action::SaveConnection);
        assert!(matches!(
            &effects[..],
            [Effect::CreateConnection { password, .. }] if password == "SUPER_SECRET_SENTINEL"
        ));
        assert!(
            model
                .connection_form
                .fields
                .iter()
                .find(|field| field.label == "password")
                .unwrap()
                .value
                .is_empty()
        );
        assert!(!format!("{:?}", model.connection_form).contains("SUPER_SECRET_SENTINEL"));
    }
}
