use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_app::data::{inspect_value, related_filter};
use dexo_driver_api::{DbValue, QueryRequest, TransactionState};

use crate::action::{Action, Effect, FocusTarget};
use crate::model::{Focus, GridModel, Model};

pub fn update(model: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Key(key) => handle_key(model, key),
        Action::Mouse(_) => Vec::new(),
        Action::Resize { width, height } => {
            model.apply_size(width, height);
            vec![Effect::PersistLayout]
        }
        Action::ConnectionChanged { name, ready } => {
            model.connection.name = name;
            model.connection.ready = ready;
            Vec::new()
        }
        Action::QueryMeta { task, columns } => {
            model.active_task = Some(task);
            model.results.set_columns(columns);
            Vec::new()
        }
        Action::QueryRows { rows, .. } => {
            model.results.append_rows(rows);
            Vec::new()
        }
        Action::QueryMessage { message, .. } => {
            model.messages.push(message);
            Vec::new()
        }
        Action::QueryFinished { .. } => {
            model.active_task = None;
            model.active_query = None;
            Vec::new()
        }
        Action::TransactionChanged(state) => {
            model.transaction = state;
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
            Vec::new()
        }
        Action::PaletteSelect => palette_select(model),
        Action::ExecuteQuery => start_query(model),
        Action::CancelQuery => cancel_query(model),
        Action::CommitTransaction => {
            if model.transaction == TransactionState::Active {
                vec![Effect::CommitTransaction]
            } else {
                Vec::new()
            }
        }
        Action::RollbackTransaction => {
            if model.transaction == TransactionState::Active
                || model.transaction == TransactionState::Failed
            {
                vec![Effect::RollbackTransaction]
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
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            if model.active_query.is_some() {
                cancel_query(model)
            } else {
                vec![Effect::Quit]
            }
        }
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => vec![Effect::Quit],
        (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
            open_palette(model);
            Vec::new()
        }
        (_, KeyCode::F(5)) => start_query(model),
        (_, KeyCode::Up) if model.focus == Focus::Results => {
            model.results.scroll_rows(-1);
            Vec::new()
        }
        (_, KeyCode::Down) if model.focus == Focus::Results => {
            model.results.scroll_rows(1);
            Vec::new()
        }
        (_, KeyCode::Left) if model.focus == Focus::Results => {
            model.results.scroll_columns(-1);
            Vec::new()
        }
        (_, KeyCode::Right) if model.focus == Focus::Results => {
            model.results.scroll_columns(1);
            Vec::new()
        }
        (_, KeyCode::PageUp) if model.focus == Focus::Results => {
            model
                .results
                .scroll_rows(-(model.results.viewport().height as i32));
            Vec::new()
        }
        (_, KeyCode::PageDown) if model.focus == Focus::Results => {
            model
                .results
                .scroll_rows(model.results.viewport().height as i32);
            Vec::new()
        }
        (_, KeyCode::Enter) if model.focus == Focus::Explorer => {
            if let Some(id) = model.explorer.selected.clone() {
                let _ = model.explorer.expand(&id);
            }
            Vec::new()
        }
        (_, KeyCode::Char('c')) if model.focus == Focus::Explorer => {
            model.explorer.copy_selected_name();
            Vec::new()
        }
        (_, KeyCode::Tab) if model.tabs.active == 2 => {
            model.schema_editor.focus_next();
            Vec::new()
        }
        _ => Vec::new(),
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
            model.palette.selected = model.palette.selected.saturating_sub(1);
            Vec::new()
        }
        KeyCode::Down => {
            let count = crate::palette::filter_entries(
                &crate::palette::palette_entries(model),
                &model.palette.query,
            )
            .len();
            if count > 0 {
                model.palette.selected = (model.palette.selected + 1).min(count - 1);
            }
            Vec::new()
        }
        KeyCode::Backspace => {
            model.palette.query.pop();
            model.palette.selected = 0;
            Vec::new()
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            model.palette.query.push(ch);
            model.palette.selected = 0;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn open_palette(model: &mut Model) {
    model.palette.open = true;
    model.palette.query.clear();
    model.palette.selected = 0;
    model.focus = Focus::Palette;
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
    vec![Effect::StartScript {
        statements,
        policy: model.script_policy,
    }]
}

fn cancel_query(model: &mut Model) -> Vec<Effect> {
    model
        .active_query
        .map(Effect::CancelQuery)
        .into_iter()
        .collect()
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
    use dexo_app::event::TaskId;
    use dexo_driver_api::DbValue;

    use super::update;
    use crate::action::Action;
    use crate::model::{Focus, Model};

    #[test]
    fn query_events_do_not_change_editor_focus() {
        let mut model = Model::fixture(Focus::Editor);
        update(
            &mut model,
            Action::QueryRows {
                task: TaskId(uuid::Uuid::nil()),
                rows: vec![vec![DbValue::I64(1)]],
            },
        );
        assert_eq!(model.focus, Focus::Editor);
        assert_eq!(model.results.row_count(), 1);
    }
}
