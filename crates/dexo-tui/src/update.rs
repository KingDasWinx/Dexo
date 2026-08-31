use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use dexo_app::data::{inspect_value, related_filter};
use dexo_driver_api::{DbValue, QueryRequest, TransactionState};

use crate::action::{Action, Effect, FocusTarget};
use crate::layout::LayoutPlan;
use crate::model::{Focus, Model};
use crate::mouse::{HitButton, HitTarget, note_click, overlay_blocks_workbench};
use ratatui::layout::Rect;

pub fn update(model: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Key(key) => handle_key(model, key),
        Action::Mouse(mouse) => handle_mouse(model, mouse),
        Action::Resize { width, height } => {
            model.apply_size(width, height);
            model.layout_dirty = true;
            // ponytail: skip-until-flush debounce; add a timer if live resize must persist mid-session.
            Vec::new()
        }
        Action::ConnectionChanged {
            name,
            ready,
            environment,
            session,
            generation,
            token,
            read_only,
            driver,
        } => {
            if let Some(pending) = model.connections.pending_connect {
                if token != pending {
                    return Vec::new();
                }
                model.connections.pending_connect = None;
            }
            model.connection.name = name.clone();
            model.connection.ready = ready;
            model.connection.environment = environment;
            model.connection.read_only = read_only;
            model.connection.driver = driver;
            model.active_session = session;
            model.session_generation = generation;
            model.connection_form.close();
            if let Some(id) = session {
                model
                    .connections
                    .upsert_session(crate::screens::connections::SessionRow {
                        id,
                        connection: name,
                        transaction: TransactionState::Idle,
                        generation,
                        environment: model.connection.environment.clone(),
                        read_only,
                        driver: model.connection.driver.clone(),
                    });
                model.connections.selected_session = Some(id);
            }
            model.explorer.clear();
            if ready {
                model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
                model.focus = Focus::Editor;
                let mut effects = Vec::new();
                if let Some(session) = session {
                    let operation = crate::runtime::OperationId::new();
                    effects.push(Effect::LoadCatalogChildren {
                        parent: None,
                        operation,
                        session,
                        generation,
                        replace_roots: true,
                        include_system: model.explorer.include_system,
                    });
                }
                if let Some(connection_id) = active_connection_uuid(model) {
                    effects.push(Effect::EnsureConnectionSql { connection_id });
                }
                effects
            } else {
                model.explorer.offline = true;
                vec![Effect::LoadOfflineCatalog {
                    connection_id: model.connection.name.clone(),
                    database_name: catalog_database(model),
                    generation,
                }]
            }
        }
        Action::ConnectionSqlReady {
            connection_id,
            files: _,
            console,
            content,
        } => {
            if active_connection_uuid(model).as_deref() != Some(connection_id.as_str()) {
                return Vec::new();
            }
            if let Some(index) = model
                .documents
                .iter()
                .position(|document| document.path.as_deref() == Some(console.as_path()))
            {
                model.active_document = index;
                if model.documents[index].connection_id.is_none() {
                    model.documents[index].connection_id = Some(connection_id);
                }
            } else {
                let title = console
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("console.sql")
                    .to_owned();
                let mut document = crate::model::EditorDocument::new_unique(
                    title,
                    Some(console),
                    Some(connection_id),
                );
                document.sql = dexo_sql::SqlDocument::new(content);
                document.saved_revision = document.sql.revision();
                model.documents.push(document);
                model.active_document = model.documents.len() - 1;
            }
            model.focus = Focus::Editor;
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
                ensure_result_tab(model, &key, index).grid.clear();
                model.results.active = index;
            }
            Vec::new()
        }
        Action::QueryMeta {
            key,
            index,
            columns,
        } => {
            if operation_matches(model, &key) {
                ensure_result_tab(model, &key, index)
                    .grid
                    .set_columns(columns);
            }
            Vec::new()
        }
        Action::QueryRows { key, index, rows } => {
            if operation_matches(model, &key) {
                ensure_result_tab(model, &key, index).grid.append_rows(rows);
            }
            Vec::new()
        }
        Action::QueryNotice {
            key,
            index,
            message,
        } => {
            if operation_matches(model, &key) {
                if let Some(tab) = model.results.tabs.get_mut(index) {
                    tab.notices.push(message.clone());
                }
                model.messages.push(message);
            }
            Vec::new()
        }
        Action::QueryResultSetFinished {
            key,
            index,
            rows_affected,
        } => {
            if let Some(tab) = result_tab_mut(model, &key, index) {
                tab.rows_affected = rows_affected;
                tab.status = crate::model::OperationStatus::Finished;
            }
            Vec::new()
        }
        Action::ScriptFinished { .. } => {
            model.active_task = None;
            model.active_query = None;
            model.active_operation = None;
            persist_history_effect(model)
        }
        Action::CheckpointTick => checkpoint_dirty(model),
        Action::TransactionChanged {
            session,
            generation,
            state,
        } => {
            if let Some(row) = model
                .connections
                .sessions
                .iter_mut()
                .find(|row| row.id == session)
            {
                row.transaction = state;
                row.generation = generation;
            }
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
        Action::TransferProgress {
            operation,
            rows,
            bytes,
        } => apply_transfer_progress(model, operation, rows, bytes),
        Action::TransferFinished { operation, message } => {
            apply_transfer_finished(model, operation, message)
        }
        Action::TransferFailed { operation, message } => {
            apply_transfer_failed(model, operation, message)
        }
        Action::Bootstrapped(state) => {
            apply_bootstrap(model, *state);
            Vec::new()
        }
        Action::SecretRequired {
            purpose,
            profile,
            buffer,
        } => {
            model.secret_prompt =
                crate::screens::secret_prompt::SecretPrompt::open_for(purpose, profile, buffer);
            Vec::new()
        }
        Action::SubmitSecret { kind } => submit_secret(model, kind),
        Action::ConfirmDeleteProfile { decision } => confirm_delete(model, decision),
        Action::OpenConnections => {
            model.connections.open = true;
            Vec::new()
        }
        Action::ConnectSelected => connect_selected(model),
        Action::EditSelectedConnection => {
            if !model.connections.open && model.focus == Focus::Explorer {
                model.connections.selected_profile = model.explorer.connection_cursor;
            }
            match model.connections.selected().cloned() {
                Some(profile) => {
                    model.connection_form =
                        crate::screens::connection::ConnectionForm::open_edit(&profile);
                }
                None => model
                    .messages
                    .push("No saved connection to edit — press n to add one.".into()),
            }
            Vec::new()
        }
        Action::DuplicateConnection => model
            .connections
            .selected()
            .map(|profile| Effect::DuplicateProfile { id: profile.id })
            .into_iter()
            .collect(),
        Action::TestConnection => test_connection(model),
        Action::DeleteConnection => {
            model.connections.delete_target = model.connections.selected().cloned();
            Vec::new()
        }
        Action::MoveConnectionGroup { group } => model
            .connections
            .selected()
            .map(|profile| Effect::MoveProfileGroup {
                id: profile.id,
                group_path: if group.is_empty() { None } else { Some(group) },
            })
            .into_iter()
            .collect(),
        Action::CloseSelectedSession => model
            .connections
            .selected()
            .and_then(|profile| {
                model
                    .connections
                    .session_for(&profile.name)
                    .map(|session| session.id)
            })
            .or(model.active_session)
            .map(|session| Effect::CloseSession { session })
            .into_iter()
            .collect(),
        Action::ProfilesLoaded(profiles) => {
            model.connections.load_profiles(profiles);
            Vec::new()
        }
        Action::ProfileSaved(profile) => {
            model.connections.load_profiles(
                model
                    .connections
                    .profiles
                    .iter()
                    .map(|row| row.profile.clone())
                    .chain(std::iter::once(profile.clone()))
                    .fold(Vec::new(), |mut acc, item| {
                        if let Some(existing) = acc.iter_mut().find(|p| p.id == item.id) {
                            *existing = item;
                        } else {
                            acc.push(item);
                        }
                        acc
                    }),
            );
            model.messages.push(format!("saved {}", profile.name));
            Vec::new()
        }
        Action::ProfileDeleted { name } => {
            model
                .connections
                .profiles
                .retain(|row| row.profile.name != name);
            let closing: Vec<_> = model
                .connections
                .sessions
                .iter()
                .filter(|row| row.connection == name)
                .map(|row| row.id)
                .collect();
            let mut effects = Vec::new();
            for session in closing {
                model.connections.remove_session(session);
                effects.push(Effect::CloseSession { session });
            }
            if model.connection.name == name {
                model.active_session = None;
                model.connection.ready = false;
                model.connection.name.clear();
                model.explorer.clear();
                model.explorer.offline = false;
                model.explorer.stale = false;
            }
            model.messages.push(format!("deleted {name}"));
            effects
        }
        Action::ConnectionTested { name, ok, message } => {
            model.messages.push(if ok {
                format!("{name} ok")
            } else {
                format!("{name}: {message}")
            });
            Vec::new()
        }
        Action::SessionClosed { session } => {
            model.connections.remove_session(session);
            if model.active_session == Some(session) {
                if let Some(next) = model.connections.sessions.first().cloned()
                    && let Some(profile) = model
                        .connections
                        .profiles
                        .iter()
                        .find(|row| row.profile.name == next.connection)
                        .map(|row| row.profile.clone())
                {
                    return activate_existing_session(model, &profile, next);
                }
                model.active_session = None;
                model.connection.ready = false;
                return enter_offline_explorer(model);
            }
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
        Action::ExecuteQuery => {
            crate::screens::workbench::execute_document(model);
            start_query(model)
        }
        Action::ExecuteStatement => {
            if model.active_document().selection().is_some() {
                crate::screens::workbench::execute_selection(model);
            } else {
                crate::screens::workbench::execute_current_statement(model);
            }
            start_query(model)
        }
        Action::ExecuteSelection => {
            crate::screens::workbench::execute_selection(model);
            start_query(model)
        }
        Action::ExecuteDocument => {
            crate::screens::workbench::execute_document(model);
            start_query(model)
        }
        Action::CancelQuery => cancel_query(model),
        Action::BeginTransaction => {
            if model.connection.read_only {
                model.messages.push("connection is read-only".into());
                return Vec::new();
            }
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
        Action::Savepoint => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Create,
        ),
        Action::RollbackSavepoint => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Rollback,
        ),
        Action::ReleaseSavepoint => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Release,
        ),
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
            crate::screens::editor::end_typing(model);
            let leaving_editor =
                model.focus == Focus::Editor && !matches!(target, FocusTarget::Editor);
            model.focus = match target {
                FocusTarget::Explorer => {
                    model.panes.explorer_visible = true;
                    Focus::Explorer
                }
                FocusTarget::Editor => Focus::Editor,
                FocusTarget::Results => {
                    model.panes.results_visible = true;
                    Focus::Results
                }
                FocusTarget::Inspector => {
                    model.panes.inspector_visible = true;
                    Focus::Inspector
                }
            };
            model.panes = model.panes.clamp(model.width, model.height);
            model.sync_grid_viewport();
            close_palette(model);
            if leaving_editor {
                checkpoint_dirty(model)
            } else {
                Vec::new()
            }
        }
        Action::ExplorerExpand => {
            if model.explorer.sidebar_focus == crate::screens::explorer::SidebarFocus::Connections {
                activate_sidebar_connection(model)
            } else {
                expand_or_open_selected(model)
            }
        }
        Action::RefreshCatalogNode => refresh_catalog(model, false),
        Action::RefreshCatalogSubtree => refresh_catalog(model, false),
        Action::RefreshCatalogAll => refresh_catalog(model, true),
        Action::CatalogLoaded {
            session,
            generation,
            parent,
            list,
            replace_roots,
            ..
        } => {
            if !catalog_generation_matches(model, &session, generation) {
                return Vec::new();
            }
            let capture = replace_roots || parent.is_none();
            if capture {
                model.explorer.replace_roots(list);
            } else if let Some(parent) = parent {
                model.explorer.apply_children(&parent, list);
            }
            catalog_followup_effects(model, capture)
        }
        Action::CatalogFailed {
            session,
            generation,
            parent,
            message,
            retryable,
            ..
        } => {
            if catalog_generation_matches(model, &session, generation) {
                if let Some(parent) = parent {
                    model.explorer.set_error(&parent, message, retryable);
                } else {
                    model.messages.push(message);
                }
            }
            Vec::new()
        }
        Action::OpenObjectInspector => open_inspector(model),
        Action::OpenObjectDdl => {
            let effects = open_inspector(model);
            model.inspector.tab = crate::screens::object_inspector::InspectorTab::Ddl;
            effects
        }
        Action::OpenObjectData => open_object_data(model),
        Action::OpenDependencies => {
            let effects = open_inspector(model);
            model.inspector.tab = crate::screens::object_inspector::InspectorTab::Dependencies;
            effects
        }
        Action::OpenDependents => {
            let effects = open_inspector(model);
            model.inspector.tab = crate::screens::object_inspector::InspectorTab::Dependencies;
            model.messages.push("dependents".into());
            effects
        }
        Action::ExplorerUp => {
            move_sidebar_selection(model, -1);
            Vec::new()
        }
        Action::ExplorerDown => {
            move_sidebar_selection(model, 1);
            Vec::new()
        }
        Action::SwitchTab { index } => {
            if index < model.tabs.titles.len() {
                model.tabs.active = index;
                model.tabs.scroll = 0;
            }
            Vec::new()
        }
        Action::NextTab => {
            if !model.tabs.titles.is_empty() {
                model.tabs.active = (model.tabs.active + 1) % model.tabs.titles.len();
                model.tabs.scroll = 0;
            }
            Vec::new()
        }
        Action::SelectDocument { index } => {
            if index < model.documents.len() {
                model.active_document = index;
                model.focus = Focus::Editor;
            }
            Vec::new()
        }
        Action::NextDocument => {
            if !model.documents.is_empty() {
                model.active_document = (model.active_document + 1) % model.documents.len();
                model.focus = Focus::Editor;
            }
            Vec::new()
        }
        Action::PrevDocument => {
            if !model.documents.is_empty() {
                model.active_document = model
                    .active_document
                    .checked_sub(1)
                    .unwrap_or(model.documents.len() - 1);
                model.focus = Focus::Editor;
            }
            Vec::new()
        }
        Action::CloseDocument => close_active_document(model),
        Action::NewDocument => {
            let connection_id = active_connection_uuid(model);
            let title = format!("query-{}.sql", model.documents.len());
            model
                .documents
                .push(crate::model::EditorDocument::new_unique(
                    title,
                    None,
                    connection_id,
                ));
            model.active_document = model.documents.len() - 1;
            Vec::new()
        }
        Action::SelectGridRow => {
            if let Some((row, _)) = model.results.selection() {
                model.results.select_row(row);
            }
            Vec::new()
        }
        Action::SelectGridColumn => {
            if let Some((_, col)) = model.results.selection() {
                model.results.select_column(col);
            }
            Vec::new()
        }
        Action::NextResultTab => {
            if !model.results.tabs.is_empty() {
                model.results.active = (model.results.active + 1) % model.results.tabs.len();
            }
            Vec::new()
        }
        Action::PrevResultTab => {
            if !model.results.tabs.is_empty() {
                model.results.active = model
                    .results
                    .active
                    .checked_sub(1)
                    .unwrap_or(model.results.tabs.len() - 1);
            }
            Vec::new()
        }
        Action::SelectResultTab { index } => {
            if index < model.results.tabs.len() {
                model.results.active = index;
            }
            model.focus = Focus::Results;
            Vec::new()
        }
        Action::InspectorNextTab => {
            model.inspector.tab = model.inspector.tab.next();
            Vec::new()
        }
        Action::NextDataPage => {
            let offset = model
                .data
                .page_offset
                .saturating_add(u64::from(model.data.page_limit));
            change_data_page(model, offset)
        }
        Action::PrevDataPage => {
            let offset = model
                .data
                .page_offset
                .saturating_sub(u64::from(model.data.page_limit));
            change_data_page(model, offset)
        }
        Action::SaveActiveDocument => save_active_document(model),
        Action::OpenDocument => {
            open_file_picker(model, crate::screens::file_picker::FilePickerMode::Open);
            Vec::new()
        }
        Action::CycleTheme => cycle_theme(model),
        Action::CycleKeymap => cycle_keymap(model),
        Action::ToggleMouse => {
            model.mouse = !model.mouse;
            model.settings.mouse = model.mouse;
            persist_settings(model);
            Vec::new()
        }
        Action::ChangeDataPage { offset } => change_data_page(model, offset),
        Action::ApplyRemoteSort | Action::ApplyRemoteFilter => apply_remote_query(model),
        Action::DataPageLoaded {
            generation,
            session,
            page,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply_page(page.clone());
                model.results.clear();
                model.results.set_columns(page.columns.clone());
                model.results.append_rows(page.rows);
                promote_remote_cells(model, &page.columns);
            }
            Vec::new()
        }
        Action::DataPageFailed {
            generation,
            message,
        } => {
            if generation == model.session_generation {
                model.data.loading = false;
                model.data.last_error = Some(message.clone());
                model.messages.push(message);
            }
            Vec::new()
        }
        Action::ValueFetched { generation, bytes } => {
            if generation == model.session_generation {
                model.data.viewer =
                    Some(crate::screens::value_viewer::view(&DbValue::Bytes(bytes)));
            }
            Vec::new()
        }
        Action::MutationsApplied {
            generation,
            session,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.data.apply();
                return reload_object_data(model);
            }
            Vec::new()
        }
        Action::MutationsFailed {
            generation,
            message,
        } => {
            if generation == model.session_generation {
                model.data.fail_apply();
                model.messages.push(message);
            }
            Vec::new()
        }
        Action::GoToDefinition => goto_definition(model),
        Action::InspectorLoaded {
            generation,
            session,
            qualified_name,
            object,
            ddl,
            dependencies,
            dependents,
            effective_privileges,
            restrictions,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                model.inspector.open = true;
                model.inspector.qualified_name = qualified_name;
                model.inspector.object = object;
                model.inspector.ddl = ddl;
                model.inspector.dependencies = dependencies;
                model.inspector.dependents = dependents;
                model.inspector.effective_privileges = effective_privileges;
                model.inspector.restrictions = restrictions;
                model.inspector.error = None;
            }
            Vec::new()
        }
        Action::InspectorFailed {
            generation,
            message,
        } => {
            if generation == model.session_generation {
                model.inspector.error = Some(message);
            }
            Vec::new()
        }
        Action::ClipboardWritten { text } => {
            model.explorer.copied = Some(text.clone());
            model.data.clipboard = text;
            Vec::new()
        }
        Action::ClipboardFailed { message } => {
            model.messages.push(message);
            Vec::new()
        }
        Action::OfflineCatalogLoaded {
            generation,
            list,
            created_at,
        } => {
            if generation == model.session_generation {
                model.explorer.replace_roots(list);
                model.explorer.offline = true;
                model.explorer.stale = true;
                if let Some(created_at) = created_at {
                    model
                        .messages
                        .push(format!("offline catalog from {created_at}"));
                }
                return catalog_followup_effects(model, false);
            }
            Vec::new()
        }
        Action::ApplyFavorites { ids } => {
            model.explorer.apply_favorites(&ids);
            Vec::new()
        }
        Action::ToggleFavorite => toggle_favorite(model),
        Action::ToggleFavoritesOnly => {
            model.explorer.favorites_only = !model.explorer.favorites_only;
            Vec::new()
        }
        Action::ToggleSystemObjects => {
            model.explorer.include_system = !model.explorer.include_system;
            if model.explorer.include_system && model.connection.ready {
                refresh_catalog(model, true)
            } else {
                Vec::new()
            }
        }
        Action::CopySimpleName => copy_selected(model, false),
        Action::CopyQualifiedName | Action::ExplorerCopyName => copy_selected(model, true),
        Action::CopyDdl => copy_ddl(model),
        Action::CopyGrid(format) => copy_grid(model, format),
        Action::OpenReview => {
            model.data.open_review();
            Vec::new()
        }
        Action::ConfirmProduction => {
            model.data.confirm_production();
            Vec::new()
        }
        Action::ApplyChanges => apply_changes(model),
        Action::FailApply => {
            model.data.fail_apply();
            Vec::new()
        }
        Action::RevertChanges => {
            model.data.revert();
            Vec::new()
        }
        Action::InspectValue => inspect_selected(model),
        Action::OpenRelated => open_related(model),
        Action::DataNavBack => data_nav_back(model),
        Action::OpenDdlPreview => open_ddl_preview(model),
        Action::ConfirmDdl => {
            model.schema_editor.confirm_typed();
            Vec::new()
        }
        Action::ApplyDdl => apply_ddl(model),
        Action::ApplyRawDdl => {
            let sql = model.active_document().text();
            if !sql.trim().is_empty() {
                model.schema_editor.apply_raw(sql);
                model.tabs.active = 2;
            } else {
                model.messages.push("no SQL to apply".into());
            }
            Vec::new()
        }
        Action::OpenSecurity => open_security(model),
        Action::SchemaFocusNext => {
            model.schema_editor.focus_next();
            Vec::new()
        }
        Action::OpenSchemaDiff => open_schema_diff(model),
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
        Action::SchemaDiffLoaded {
            from_label,
            to_label,
            ordered,
        } => {
            let left = model.schema_diff.left.clone();
            let right = model.schema_diff.right.clone();
            model.schema_diff = crate::screens::schema_diff::SchemaDiffScreen::from_ordered(
                from_label, to_label, &ordered,
            );
            model.schema_diff.left = left;
            model.schema_diff.right = right;
            model.schema_diff.loading = false;
            model.schema_diff.source_prompt = false;
            Vec::new()
        }
        Action::SchemaDiffFailed { message } => {
            model.schema_diff.loading = false;
            model.schema_diff.error = Some(message);
            Vec::new()
        }
        Action::SecurityLoaded { principals, grants } => {
            model.security.principals = principals;
            model.security.grants = grants;
            model.security.selected = 0;
            Vec::new()
        }
        Action::SecurityFailed { message } => {
            model.messages.push(message);
            Vec::new()
        }
        Action::OpenTransfer => {
            open_transfer(model, crate::screens::transfer::TransferMode::Export)
        }
        Action::OpenBackup => open_transfer(model, crate::screens::transfer::TransferMode::Backup),
        Action::OpenRestore => {
            open_transfer(model, crate::screens::transfer::TransferMode::Restore)
        }
        Action::OpenExplain => {
            model.tabs.active = 4;
            explain_effect(model, false)
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
            explain_effect(model, true)
        }
        Action::OpenAdmin => {
            model.admin.open = true;
            model
                .active_session
                .map(|session| Effect::LoadAdminSessions {
                    session,
                    generation: model.session_generation,
                })
                .into_iter()
                .collect()
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
            if model.admin.confirm_target.is_empty() {
                model.admin.confirm_target = model
                    .admin
                    .sessions
                    .first()
                    .map(|session| session.id.clone())
                    .unwrap_or_default();
            }
            match model.active_session {
                Some(session) if !model.admin.confirm_target.is_empty() => {
                    vec![Effect::AdminTerminate {
                        session,
                        target: model.admin.confirm_target.clone(),
                    }]
                }
                _ => Vec::new(),
            }
        }
        Action::OpenMcpProfiles => {
            model.mcp_profiles.open = true;
            vec![Effect::LoadMcpProfiles]
        }
        Action::ConfirmMcpEnable => {
            model.mcp_profiles.confirm_enable();
            vec![Effect::EnableMcpProfile {
                name: model.mcp_profiles.name.clone(),
            }]
        }
        Action::RevokeAllMcpGrants => {
            model.mcp_audit.open = false;
            model.mcp_profiles.open = true;
            model.mcp_profiles.confirm_revoke = true;
            model.mcp_profiles.preview = "confirm revoke all grants".into();
            Vec::new()
        }
        Action::McpGrantsRevoked { count } => {
            model.mcp_profiles.grants.clear();
            model.mcp_profiles.confirm_revoke = false;
            model.mcp_profiles.preview = format!("revoked {count} grants");
            Vec::new()
        }
        Action::McpRevokeFailed { message } => {
            model.mcp_profiles.preview = message;
            Vec::new()
        }
        Action::OpenSettings => {
            model.settings.open = true;
            model.settings.mouse = model.mouse;
            Vec::new()
        }
        Action::ConfirmResetSettings => {
            if !model.settings.open {
                model.settings.open = true;
            }
            if !model.settings.confirm_reset {
                model.settings.confirm_reset = true;
            } else {
                model.settings.reset();
                persist_settings(model);
            }
            Vec::new()
        }
        Action::OpenRecovery => {
            model.recovery.open = true;
            Vec::new()
        }
        Action::ConfirmRecover => {
            let checkpoints = model.recovery.restore_documents();
            if !checkpoints.is_empty() {
                model.documents = checkpoints
                    .into_iter()
                    .map(|(id, title, content)| {
                        let mut document = crate::model::EditorDocument::with_text(&content);
                        document.id = id;
                        document.title = title;
                        document
                    })
                    .collect();
                model.active_document = 0;
            }
            model.recovery.recover();
            Vec::new()
        }
        Action::ConfirmDiscardRecovery => {
            if !model.recovery.open {
                model.recovery.open = true;
            }
            if !model.recovery.confirm_discard {
                model.recovery.confirm_discard = true;
            } else {
                model.recovery.discard();
            }
            Vec::new()
        }
        Action::OpenMcpAudit => {
            model.mcp_audit.open = true;
            vec![Effect::LoadMcpAudit]
        }
        Action::OpenDiagnostics => {
            let bundle = diagnostics_bundle(model);
            model.diagnostics.open = true;
            model.diagnostics.writing = false;
            model.diagnostics.error = None;
            model.diagnostics.preview = format!(
                "Dexo never uploads this bundle automatically.\n\n{}",
                bundle.preview
            );
            model.diagnostic_preview = Some(model.diagnostics.preview.clone());
            Vec::new()
        }
        Action::DiagnosticsWritten { path } => {
            model.diagnostics.writing = false;
            model.diagnostics.path = Some(path);
            model.diagnostics.error = None;
            Vec::new()
        }
        Action::DiagnosticsFailed { message } => {
            model.diagnostics.writing = false;
            model.diagnostics.error = Some(message);
            Vec::new()
        }
        Action::RefreshSqlIntelligence => {
            crate::screens::editor::refresh_intelligence(model, true);
            Vec::new()
        }
        Action::FormatSql => {
            crate::screens::editor::apply_format(model);
            Vec::new()
        }
        Action::AcceptCompletion => {
            crate::screens::editor::accept_completion(model);
            Vec::new()
        }
        Action::InsertSnippet => {
            crate::screens::editor::insert_active_snippet(model);
            Vec::new()
        }
        Action::SubmitParameters => submit_parameter_prompt(model),
        Action::SearchHistory => {
            model.editor.history_open = true;
            model.editor.history_selected = 0;
            vec![Effect::LoadHistory {
                connection_id: None,
            }]
        }
        Action::ClearHistory => confirm_clear_history(model),
        Action::HistoryLoaded(entries) => {
            model.editor.history = entries;
            model.editor.history_open = true;
            model.editor.history_selected = 0;
            Vec::new()
        }
        Action::HistoryPick => {
            if crate::screens::editor::pick_history(model) {
                crate::screens::workbench::execute_document(model);
                start_query(model)
            } else {
                Vec::new()
            }
        }
        Action::SnippetsLoaded(snippets) => {
            model.editor.snippet_pending = false;
            model.editor.snippets = snippets;
            model.editor.snippet_open = !model.editor.snippets.is_empty();
            if model.editor.snippets.is_empty() {
                model.messages.push("no snippets available".into());
            }
            Vec::new()
        }
        Action::SnippetPick => {
            crate::screens::editor::insert_snippet_at(model, model.editor.snippet_selected);
            Vec::new()
        }
        Action::DdlPreviewed {
            sql,
            confirmation,
            warnings,
        } => {
            let preview = dexo_app::schema::DdlPreview {
                plan: {
                    let mut plan = dexo_driver_api::DdlPlan::default();
                    if !sql.is_empty() {
                        plan.push(sql, false);
                    }
                    plan.warnings = warnings;
                    plan
                },
                risk: dexo_driver_api::ChangeRisk::default(),
                dependents: Vec::new(),
                grants: Vec::new(),
                confirmation,
                warnings: Vec::new(),
            };
            model.schema_editor.open_preview(preview);
            Vec::new()
        }
        Action::SchemaApplied { message } => {
            model.messages.push(message);
            model.schema_editor.preview = None;
            Vec::new()
        }
        Action::ExplainLoaded { plan } => {
            let previous = model.explain.plan.clone();
            model.explain.set_plan(*plan, previous.as_ref());
            model.tabs.active = 4;
            Vec::new()
        }
        Action::AdminSessionsLoaded {
            sessions,
            captured_at,
            blocking,
        } => {
            model.admin.sessions = sessions;
            model.admin.captured_at = captured_at;
            model.admin.blocking = blocking;
            model.admin.open = true;
            Vec::new()
        }
        Action::DiagnosticsReady { preview } => {
            model.diagnostic_preview = Some(preview.clone());
            model.diagnostics.preview = preview.clone();
            model.diagnostics.open = true;
            model.messages.push(preview);
            Vec::new()
        }
        Action::McpProfilesLoaded { profiles } => {
            model.mcp_profiles.load_profiles(profiles);
            Vec::new()
        }
        Action::McpAuditLoaded { events } => {
            model.mcp_audit.events = events;
            Vec::new()
        }
        Action::DocumentLoaded { document, content } => {
            if let Some(doc) = model.documents.iter_mut().find(|item| item.id == document) {
                *doc = crate::model::EditorDocument::with_text(content);
                doc.id = document;
            }
            Vec::new()
        }
        Action::DocumentAutosaved { id, revision } => {
            if let Some(document) = model
                .documents
                .iter_mut()
                .find(|document| document.id == id)
            {
                document.saved_revision = revision;
            }
            Vec::new()
        }
        Action::DocumentSaved { document, revision } => {
            let current_revision = model
                .documents
                .iter_mut()
                .find(|candidate| candidate.id == document)
                .map(|candidate| {
                    let current_revision = candidate.sql.revision();
                    if revision <= current_revision && revision > candidate.saved_revision {
                        candidate.saved_revision = revision;
                    }
                    current_revision
                });

            if let Some(pending) = &model.pending_document_close
                && pending.document == document
            {
                let should_close =
                    pending.revision == revision && current_revision == Some(revision);
                model.pending_document_close = None;
                if should_close
                    && let Some(index) = model
                        .documents
                        .iter()
                        .position(|candidate| candidate.id == document)
                {
                    remove_document(model, index);
                }
            }
            Vec::new()
        }
        Action::DocumentConflict { path } => {
            // The write never landed, so any tab waiting on it stays open.
            model.pending_document_close = None;
            model.messages.push(format!("file changed on disk: {path}"));
            Vec::new()
        }
        Action::ResultsUp => {
            model.results.move_cursor_row(-1, false);
            Vec::new()
        }
        Action::ResultsDown => {
            model.results.move_cursor_row(1, false);
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
            let height = model.results.viewport().height as i32;
            model.results.move_cursor_row(-height.max(1), false);
            Vec::new()
        }
        Action::ResultsPageDown => {
            let height = model.results.viewport().height as i32;
            model.results.move_cursor_row(height.max(1), false);
            Vec::new()
        }
        Action::ResultsTop => {
            model.results.ensure_cursor();
            if let Some((_, col)) = model.results.selection() {
                model.results.select_cell(0, col);
            }
            let offset = model.results.viewport().row_offset as i32;
            model.results.scroll_rows(-offset);
            Vec::new()
        }
        Action::ResultsExtendUp => {
            model.results.move_cursor_row(-1, true);
            Vec::new()
        }
        Action::ResultsExtendDown => {
            model.results.move_cursor_row(1, true);
            Vec::new()
        }
        Action::OpenResultsMenu => {
            open_results_menu(model);
            Vec::new()
        }
        Action::ToggleResultsPick => {
            model.results.toggle_picked_row();
            Vec::new()
        }
        Action::ToggleHelp => {
            toggle_help(model);
            Vec::new()
        }
        Action::CycleLayout => {
            apply_layout_preset(model, model.layout_preset.next());
            Vec::new()
        }
        Action::ResetLayout => {
            apply_layout_preset(model, crate::layout::LayoutPreset::Normal);
            Vec::new()
        }
        Action::HideInspector => {
            model.panes.inspector_visible = !model.panes.inspector_visible;
            if !model.panes.inspector_visible && model.focus == Focus::Inspector {
                model.focus = Focus::Editor;
            }
            model.panes = model.panes.clamp(model.width, model.height);
            model.layout_dirty = true;
            model.sync_grid_viewport();
            Vec::new()
        }
        Action::LayoutResultsFocus => {
            apply_layout_preset(model, crate::layout::LayoutPreset::ResultsWide);
            model.focus = Focus::Results;
            Vec::new()
        }
        Action::GrowResults => {
            adjust_results_height(model, 2);
            Vec::new()
        }
        Action::ShrinkResults => {
            adjust_results_height(model, -2);
            Vec::new()
        }
        Action::GrowExplorer => {
            adjust_explorer_width(model, 2);
            Vec::new()
        }
        Action::ShrinkExplorer => {
            adjust_explorer_width(model, -2);
            Vec::new()
        }
        Action::GrowInspector => {
            adjust_inspector_width(model, 2);
            Vec::new()
        }
        Action::ShrinkInspector => {
            adjust_inspector_width(model, -2);
            Vec::new()
        }
        Action::Quit => {
            let mut effects = checkpoint_dirty(model);
            effects.push(persist_layout_effect(model));
            effects.push(Effect::Shutdown);
            effects
        }
        Action::OpenProjects => {
            model.projects.open = true;
            vec![Effect::ListProjects]
        }
        Action::SwitchProject { name } => switch_project(model, name),
        Action::ProjectSwitchTarget(project) => start_switch(model, project),
        Action::CreateProject { name } => {
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.name_input.clear();
            vec![Effect::CreateProject { name }]
        }
        Action::RenameProject { name } => model
            .projects
            .selected()
            .map(|project| Effect::RenameProject {
                id: project.id.0.to_string(),
                name,
            })
            .into_iter()
            .collect(),
        Action::DeleteProject => model
            .projects
            .selected()
            .map(|project| Effect::PreviewProjectDelete {
                id: project.id.0.to_string(),
            })
            .into_iter()
            .collect(),
        Action::ConfirmProjectDelete => confirm_project_delete(model),
        Action::ConfirmSwitchDirty => complete_switch_stage(model),
        Action::CancelProjectSwitch => {
            model.projects.pending = None;
            Vec::new()
        }
        Action::ProjectsLoaded(projects) => {
            model.projects.load(projects);
            Vec::new()
        }
        Action::ProjectLoaded {
            project,
            documents,
            layout,
        } => {
            apply_loaded_project(model, project, documents, layout);
            Vec::new()
        }
        Action::ProjectDeleted { name } => {
            model.projects.delete = None;
            model.projects.recents.retain(|item| item != &name);
            if model.project == name {
                model.project.clear();
                model.project_id.clear();
            }
            Vec::new()
        }
        Action::DocumentsFlushed => {
            for document in &mut model.documents {
                document.saved_revision = document.sql.revision();
            }
            complete_switch_stage(model)
        }
        Action::LayoutPersisted => {
            model.layout_dirty = false;
            complete_switch_stage(model)
        }
        Action::ProjectSessionsClosed => {
            model.active_session = None;
            complete_switch_stage(model)
        }
        Action::ProjectSwitchFailed { message } => {
            model.projects.pending = None;
            model.messages.push(message);
            Vec::new()
        }
        Action::ProjectDeletePreviewed { project, preview } => {
            model.projects.mode = crate::screens::projects::ProjectsMode::DeleteConfirm;
            model.projects.delete = Some(crate::screens::projects::ProjectDeletePrompt {
                project,
                preview,
                delete_connections: false,
                typed: String::new(),
            });
            Vec::new()
        }
        Action::OpenConfigTransfer => {
            model.config_transfer.open = true;
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Export;
            Vec::new()
        }
        Action::ExportConfig { path } => {
            model.config_transfer.path = path.clone();
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Export;
            vec![Effect::ExportConfig { path }]
        }
        Action::ImportConfig { path } => {
            model.config_transfer.path = path.clone();
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Import;
            vec![Effect::ImportConfig { path }]
        }
        Action::ApplyConfigImport => {
            let path = model.config_transfer.path.clone();
            let resolutions = model.config_transfer.resolutions.clone();
            vec![Effect::ApplyConfigImport { path, resolutions }]
        }
        Action::ConfigPreviewed {
            conflicts,
            needing_secret,
        } => {
            model.config_transfer.preview = Some(dexo_storage::ImportPreview {
                conflicts,
                connections_needing_secret: needing_secret,
            });
            Vec::new()
        }
        Action::ConfigImported { needing_secret } => {
            model.config_transfer.needing_secret = needing_secret;
            model.config_transfer.message = Some("ok".into());
            Vec::new()
        }
    }
}

fn handle_mouse(model: &mut Model, mouse: MouseEvent) -> Vec<Effect> {
    if !model.mouse {
        return Vec::new();
    }
    match mouse.kind {
        MouseEventKind::Down(_) => handle_mouse_down(model, mouse),
        MouseEventKind::ScrollUp => handle_mouse_scroll(model, mouse, -1),
        MouseEventKind::ScrollDown => handle_mouse_scroll(model, mouse, 1),
        MouseEventKind::ScrollLeft => update(model, Action::ResultsLeft),
        MouseEventKind::ScrollRight => update(model, Action::ResultsRight),
        _ => Vec::new(),
    }
}

fn handle_mouse_down(model: &mut Model, mouse: MouseEvent) -> Vec<Effect> {
    let hit = model.hits.at(mouse.column, mouse.row);
    let doubled = hit.map(|target| note_click(model, target)).unwrap_or(false);
    if model.palette.open {
        return mouse_palette(model, hit);
    }
    if model.help.open {
        return mouse_help(model, hit);
    }
    if model.results_menu.open {
        return mouse_results_menu(model, hit);
    }
    if model.secret_prompt.open {
        return mouse_secret(model, hit);
    }
    if model.transaction_prompt.open {
        return mouse_transaction(model, hit);
    }
    if model.data.query_prompt.open {
        return mouse_data_query(model, hit);
    }
    if model.projects.open {
        return mouse_projects(model, hit, doubled);
    }
    if model.config_transfer.open {
        return mouse_config_transfer(model, hit);
    }
    if model.connections.open && !model.connection_form.open {
        return mouse_connections(model, hit, doubled);
    }
    if model.connection_form.open {
        return mouse_connection_form(model, hit);
    }
    if model.file_picker.open {
        return mouse_file_picker(model, hit, doubled);
    }
    if model.editor.history_open {
        return mouse_history(model, hit);
    }
    if model.editor.snippet_open {
        return mouse_snippets(model, hit);
    }
    if model.editor.parameter_prompt {
        return mouse_parameters(model, hit);
    }
    if model.admin.open {
        return mouse_admin(model, hit);
    }
    if model.schema_editor.preview.is_some() {
        return mouse_ddl_preview(model, hit);
    }
    if model.schema_diff.open {
        return mouse_schema_diff(model, hit);
    }
    if model.security.open {
        return mouse_security(model, hit, doubled);
    }
    if model.diagnostics.open {
        return mouse_diagnostics(model, hit);
    }
    if model.transfer.open {
        return mouse_transfer(model, hit);
    }
    if model.data.review.is_some() {
        return mouse_review(model, hit);
    }
    if model.mcp_profiles.open {
        return mouse_mcp_profiles(model, hit);
    }
    if model.settings.open {
        return mouse_settings(model, hit);
    }
    if model.recovery.open {
        return mouse_recovery(model, hit);
    }
    if model.mcp_audit.open {
        return mouse_mcp_audit(model, hit);
    }
    if model.editor.completion_open
        && let Some(HitTarget::ListRow(index)) = hit
    {
        model.editor.completion_selected = index;
        return update(model, Action::AcceptCompletion);
    }
    if overlay_blocks_workbench(model) {
        return Vec::new();
    }
    mouse_workbench(model, mouse, hit, doubled)
}

fn mouse_palette(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.palette.selected = index;
            palette_select(model)
        }
        Some(HitTarget::Overlay) => Vec::new(),
        _ => {
            close_palette(model);
            Vec::new()
        }
    }
}

fn mouse_help(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    if matches!(
        hit,
        Some(HitTarget::Overlay | HitTarget::Button(HitButton::Close)) | None
    ) {
        model.help.open = false;
        model.help.scroll = 0;
    }
    Vec::new()
}

fn mouse_results_menu(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.results_menu.selected = index;
            pick_results_menu(model)
        }
        Some(HitTarget::Overlay) => Vec::new(),
        _ => Vec::new(),
    }
}

fn mouse_secret(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Session)) => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::SessionOnly,
            },
        ),
        Some(HitTarget::Button(HitButton::Keychain)) => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::SaveToKeychain,
            },
        ),
        Some(HitTarget::Button(HitButton::Cancel) | HitTarget::Overlay) => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::Cancel,
            },
        ),
        _ => Vec::new(),
    }
}

fn mouse_transaction(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FormField(_)) => {
            model.transaction_prompt.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => submit_savepoint_prompt(model),
        Some(HitTarget::FooterCancel) => {
            model.transaction_prompt.open = false;
            model.transaction_prompt.error = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_data_query(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FormField(0)) => {
            model.data.query_prompt.focus_value = false;
            model.data.query_prompt.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FormField(_)) => {
            model.data.query_prompt.focus_value = true;
            model.data.query_prompt.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ToggleDescending)) => {
            model.data.query_prompt.descending = !model.data.query_prompt.descending;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => submit_data_query_prompt(model),
        Some(HitTarget::FooterCancel) => {
            model.data.query_prompt.open = false;
            model.data.query_prompt.error = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_projects(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if index < model.projects.list.len() {
                model.projects.selected = index;
            }
            if doubled {
                choose_project_intent(model)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::FormField(_)) => {
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => submit_project_name(model),
        Some(HitTarget::FooterCancel) => {
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.name_input.clear();
            model.projects.error = None;
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ConfirmDelete)) => {
            update(model, Action::ConfirmProjectDelete)
        }
        Some(HitTarget::Button(HitButton::ToggleConnections)) => {
            if let Some(delete) = &mut model.projects.delete {
                delete.delete_connections = !delete.delete_connections;
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ConfirmDirty)) => {
            update(model, Action::ConfirmSwitchDirty)
        }
        _ => Vec::new(),
    }
}

fn mouse_config_transfer(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if let Some(preview) = &model.config_transfer.preview
                && let Some(name) = preview.conflicts.get(index).cloned()
            {
                let next = match model.config_transfer.resolutions.get(&name) {
                    Some(dexo_storage::ImportResolution::Skip) | None => {
                        dexo_storage::ImportResolution::Replace
                    }
                    Some(dexo_storage::ImportResolution::Replace) => {
                        dexo_storage::ImportResolution::Rename(format!("{name}-2"))
                    }
                    Some(dexo_storage::ImportResolution::Rename(_)) => {
                        dexo_storage::ImportResolution::Skip
                    }
                };
                model.config_transfer.resolutions.insert(name, next);
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Apply) | HitTarget::FooterSubmit) => {
            update(model, Action::ApplyConfigImport)
        }
        _ => Vec::new(),
    }
}

fn mouse_connections(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    if model.connections.delete_target.is_some() {
        return match hit {
            Some(HitTarget::Button(HitButton::KeepSecrets)) => update(
                model,
                Action::ConfirmDeleteProfile {
                    decision: crate::screens::secret_prompt::DeleteSecretDecision::KeepSecrets,
                },
            ),
            Some(HitTarget::Button(HitButton::DeleteSecrets)) => update(
                model,
                Action::ConfirmDeleteProfile {
                    decision: crate::screens::secret_prompt::DeleteSecretDecision::DeleteSecrets,
                },
            ),
            Some(HitTarget::Button(HitButton::Cancel)) => {
                model.connections.delete_target = None;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if index < model.connections.profiles.len() {
                model.connections.selected_profile = index;
            }
            if doubled {
                choose_connection_intent(model)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::Button(HitButton::New)) => update(model, Action::OpenConnectionForm),
        Some(HitTarget::Button(HitButton::Edit)) => {
            if let Some(profile) = model.connections.selected().cloned() {
                model.connection_form =
                    crate::screens::connection::ConnectionForm::open_edit(&profile);
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Duplicate)) => update(model, Action::DuplicateConnection),
        Some(HitTarget::Button(HitButton::Test)) => update(model, Action::TestConnection),
        Some(HitTarget::Button(HitButton::Delete)) => update(model, Action::DeleteConnection),
        Some(HitTarget::Button(HitButton::CloseSession)) => {
            update(model, Action::CloseSelectedSession)
        }
        _ => Vec::new(),
    }
}

fn mouse_connection_form(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::ToggleAdvanced)) => {
            model.connection_form.focus = model.connection_form.advanced_focus_index();
            model.connection_form.toggle_advanced();
            Vec::new()
        }

        Some(HitTarget::FormField(index)) => {
            if index < model.connection_form.fields.len() {
                model.connection_form.focus = index;
            }
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => save_connection(model),
        Some(HitTarget::FooterCancel) => {
            model.connection_form.close();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::CycleDriver)) => {
            model.connection_form.cycle_driver(1);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_file_picker(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if index < model.file_picker.entries.len() {
                model.file_picker.selected = index;
                model.file_picker.focus = crate::screens::file_picker::FilePickerFocus::List;
            }
            if doubled {
                if model.file_picker.activate_selected().is_some() {
                    file_picker_submit(model)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::Button(HitButton::ParentDir)) => {
            model.file_picker.parent();
            Vec::new()
        }
        Some(HitTarget::FormField(_)) => {
            model.file_picker.focus = crate::screens::file_picker::FilePickerFocus::Name;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => file_picker_submit(model),
        Some(HitTarget::FooterCancel) => {
            model.file_picker.open = false;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_history(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    if model.editor.history_confirm_clear {
        return match hit {
            Some(HitTarget::Button(HitButton::Confirm) | HitTarget::Overlay) => {
                confirm_clear_history(model)
            }
            _ => Vec::new(),
        };
    }
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.editor.history_selected = index;
            update(model, Action::HistoryPick)
        }
        _ => Vec::new(),
    }
}

fn mouse_snippets(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.editor.snippet_selected = index;
            update(model, Action::SnippetPick)
        }
        _ => Vec::new(),
    }
}

fn mouse_parameters(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FooterSubmit | HitTarget::FormField(_)) => {
            update(model, Action::SubmitParameters)
        }
        Some(HitTarget::FooterCancel) => {
            model.editor.parameter_prompt = false;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_admin(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Pause)) => update(model, Action::AdminPause),
        Some(HitTarget::Button(HitButton::Resume)) => update(model, Action::AdminResume),
        Some(HitTarget::Button(HitButton::Confirm)) => update(model, Action::ConfirmAdmin),
        _ => Vec::new(),
    }
}

fn mouse_ddl_preview(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(
            HitTarget::Button(HitButton::Apply | HitButton::Confirm) | HitTarget::FooterSubmit,
        ) => apply_ddl(model),
        Some(HitTarget::Button(HitButton::Cancel)) => {
            model.schema_editor.preview = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_schema_diff(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::ToggleAdded)) => {
            model.schema_diff.toggle_added();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ToggleRemoved)) => {
            model.schema_diff.toggle_removed();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ToggleChanged)) => {
            model.schema_diff.toggle_changed();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ConfirmDiff)) => {
            model.schema_diff.confirm();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ApplyDiff)) => {
            model.schema_diff.apply();
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_security(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if index < model.security.principals.len() {
                model.security.selected = index;
            }
            if doubled {
                open_security_change_preview(model)
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn mouse_diagnostics(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Export) | HitTarget::Overlay)
            if !model.diagnostics.writing =>
        {
            open_diagnostics_picker(model)
        }
        _ => Vec::new(),
    }
}

fn mouse_transfer(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FormField(_)) => {
            model.transfer.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => run_transfer(model),
        Some(HitTarget::FooterCancel) => {
            model.transfer.open = false;
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Confirm)) => {
            model.transfer.confirm_restore = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_review(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::ConfirmProduction)) => {
            model.data.confirm_production();
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Apply) | HitTarget::FooterSubmit) => {
            update(model, Action::ApplyChanges)
        }
        _ => Vec::new(),
    }
}

fn mouse_mcp_profiles(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            while model.mcp_profiles.selected > index {
                model.mcp_profiles.select_previous();
            }
            while model.mcp_profiles.selected < index
                && model.mcp_profiles.selected + 1 < model.mcp_profiles.profiles.len()
            {
                model.mcp_profiles.select_next();
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Revoke)) => {
            if model.mcp_profiles.confirm_revoke {
                vec![Effect::RevokeAllMcpGrants]
            } else {
                model.mcp_profiles.revoke_all();
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn mouse_settings(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Theme)) => update(model, Action::CycleTheme),
        Some(HitTarget::Button(HitButton::Keymap)) => update(model, Action::CycleKeymap),
        Some(HitTarget::Button(HitButton::Mouse)) => update(model, Action::ToggleMouse),
        Some(HitTarget::Button(HitButton::Reset)) => update(model, Action::ConfirmResetSettings),
        _ => Vec::new(),
    }
}

fn mouse_recovery(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Recover)) => update(model, Action::ConfirmRecover),
        Some(HitTarget::Button(HitButton::Discard)) => {
            update(model, Action::ConfirmDiscardRecovery)
        }
        _ => Vec::new(),
    }
}

fn mouse_mcp_audit(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::Button(HitButton::Revoke)) => update(model, Action::RevokeAllMcpGrants),
        _ => Vec::new(),
    }
}

fn mouse_workbench(
    model: &mut Model,
    mouse: MouseEvent,
    hit: Option<HitTarget>,
    doubled: bool,
) -> Vec<Effect> {
    let extend = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let pick = mouse.modifiers.contains(KeyModifiers::CONTROL);
    match hit {
        Some(HitTarget::WorkbenchTab(index)) => update(model, Action::SwitchTab { index }),
        Some(HitTarget::ResultTab(index)) => update(model, Action::SelectResultTab { index }),
        Some(HitTarget::DocumentTab(index)) => update(model, Action::SelectDocument { index }),
        Some(HitTarget::Inspector) => update(model, Action::Focus(FocusTarget::Inspector)),
        Some(HitTarget::Explorer) => update(model, Action::Focus(FocusTarget::Explorer)),
        Some(HitTarget::ExplorerNode(index)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Explorer;
            model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
            let ids = model.explorer.visible_ids();
            if let Some(id) = ids.get(index).cloned() {
                model.explorer.select(id);
            }
            if doubled {
                expand_or_open_selected(model)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::SidebarConnection(index)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Explorer;
            model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Connections;
            model.explorer.connection_cursor = index;
            activate_sidebar_connection(model)
        }
        Some(HitTarget::Editor) => {
            close_palette(model);
            model.focus = Focus::Editor;
            let plan = LayoutPlan::for_area_with_document_tabs(
                Rect::new(0, 0, model.width, model.height),
                Some(&model.panes),
                model.tabs.active == 0,
            );
            if model.tabs.active == 0
                && let Some(index) = crate::widgets::editor::char_index_at(
                    model,
                    plan.content,
                    mouse.column,
                    mouse.row,
                )
            {
                let _ = model.active_document_mut().sql.set_cursor(index);
            }
            Vec::new()
        }
        Some(HitTarget::FormField(index)) => {
            model.focus = Focus::Editor;
            if index < model.schema_editor.fields.len() {
                model.schema_editor.focus = index;
            }
            Vec::new()
        }
        Some(HitTarget::GridHeader(col)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Results;
            model.results.select_column(col);
            Vec::new()
        }
        Some(HitTarget::GridCell { row, col }) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Results;
            if extend {
                click_results_row(model, row, true);
                if let crate::model::GridSelection::Range { start, .. } = model.results.kind {
                    model.results.select_range(start, (row, col));
                }
            } else {
                model.results.select_cell(row, col);
            }
            if pick {
                update(model, Action::ToggleResultsPick)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::GridRow(row)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Results;
            click_results_row(model, row, extend);
            if pick {
                update(model, Action::ToggleResultsPick)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::Grid) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Results;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_mouse_scroll(model: &mut Model, mouse: MouseEvent, delta: i32) -> Vec<Effect> {
    if model.palette.open {
        move_palette_selection(model, delta as isize);
        return Vec::new();
    }
    if model.help.open {
        if delta < 0 {
            model.help.scroll = model.help.scroll.saturating_sub(1);
        } else {
            model.help.scroll = model.help.scroll.saturating_add(1);
        }
        return Vec::new();
    }
    if model.results_menu.open {
        let count = crate::palette::results_menu_items().len();
        if count > 0 {
            if delta < 0 {
                model.results_menu.selected = model.results_menu.selected.saturating_sub(1);
            } else {
                model.results_menu.selected =
                    (model.results_menu.selected + 1).min(count.saturating_sub(1));
            }
        }
        return Vec::new();
    }
    if model.file_picker.open {
        model
            .file_picker
            .move_selection(delta, file_picker_rows(model));
        return Vec::new();
    }
    if model.editor.completion_open {
        crate::screens::editor::move_completion(model, delta);
        return Vec::new();
    }
    if model.editor.history_open {
        if delta < 0 {
            model.editor.history_selected = model.editor.history_selected.saturating_sub(1);
        } else if !model.editor.history.is_empty() {
            model.editor.history_selected = (model.editor.history_selected + 1)
                .min(model.editor.history.len().saturating_sub(1));
        }
        return Vec::new();
    }
    if model.editor.snippet_open {
        if delta < 0 {
            model.editor.snippet_selected = model.editor.snippet_selected.saturating_sub(1);
        } else if !model.editor.snippets.is_empty() {
            model.editor.snippet_selected = (model.editor.snippet_selected + 1)
                .min(model.editor.snippets.len().saturating_sub(1));
        }
        return Vec::new();
    }
    if model.mcp_profiles.open {
        if delta < 0 {
            model.mcp_profiles.select_previous();
        } else {
            model.mcp_profiles.select_next();
        }
        return Vec::new();
    }
    if model.connections.open && !model.connection_form.open {
        if delta < 0 {
            model.connections.selected_profile =
                model.connections.selected_profile.saturating_sub(1);
        } else if model.connections.selected_profile + 1 < model.connections.profiles.len() {
            model.connections.selected_profile += 1;
        }
        return Vec::new();
    }
    if model.projects.open {
        if delta < 0 {
            model.projects.selected = model.projects.selected.saturating_sub(1);
        } else if model.projects.selected + 1 < model.projects.list.len() {
            model.projects.selected += 1;
        }
        return Vec::new();
    }
    if overlay_blocks_workbench(model) {
        return Vec::new();
    }
    match model.hits.at(mouse.column, mouse.row) {
        Some(
            HitTarget::Explorer | HitTarget::ExplorerNode(_) | HitTarget::SidebarConnection(_),
        ) => {
            if delta < 0 {
                update(model, Action::ExplorerUp)
            } else {
                update(model, Action::ExplorerDown)
            }
        }
        Some(
            HitTarget::Grid
            | HitTarget::GridRow(_)
            | HitTarget::GridCell { .. }
            | HitTarget::GridHeader(_)
            | HitTarget::ResultTab(_),
        ) => {
            if delta < 0 {
                update(model, Action::ResultsUp)
            } else {
                update(model, Action::ResultsDown)
            }
        }
        Some(HitTarget::Editor) => {
            let doc = model.active_document_mut();
            if delta < 0 {
                doc.viewport_line = doc.viewport_line.saturating_sub(1);
            } else {
                doc.viewport_line = doc.viewport_line.saturating_add(1);
            }
            Vec::new()
        }
        _ => match model.focus {
            Focus::Explorer => {
                if delta < 0 {
                    update(model, Action::ExplorerUp)
                } else {
                    update(model, Action::ExplorerDown)
                }
            }
            Focus::Results => {
                if delta < 0 {
                    update(model, Action::ResultsUp)
                } else {
                    update(model, Action::ResultsDown)
                }
            }
            Focus::Editor | Focus::Palette => {
                let doc = model.active_document_mut();
                if delta < 0 {
                    doc.viewport_line = doc.viewport_line.saturating_sub(1);
                } else {
                    doc.viewport_line = doc.viewport_line.saturating_add(1);
                }
                Vec::new()
            }
            Focus::Inspector => Vec::new(),
        },
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if key.kind != KeyEventKind::Press {
        return Vec::new();
    }
    if model.palette.open {
        return handle_palette_key(model, key);
    }
    if model.help.open {
        return handle_help_key(model, key);
    }
    if model.results_menu.open {
        return handle_results_menu_key(model, key);
    }
    if model.secret_prompt.open {
        return handle_secret_prompt_key(model, key);
    }
    if model.transaction_prompt.open {
        return handle_transaction_prompt_key(model, key);
    }
    if model.data.query_prompt.open {
        return handle_data_query_prompt_key(model, key);
    }
    if model.projects.open {
        return handle_projects_key(model, key);
    }
    if model.config_transfer.open {
        return handle_config_transfer_key(model, key);
    }
    if model.connections.open && !model.connection_form.open {
        return handle_connections_key(model, key);
    }
    if model.connection_form.open {
        return handle_connection_form_key(model, key);
    }
    if model.file_picker.open {
        return handle_file_picker_key(model, key);
    }
    if model.editor.history_open {
        return handle_history_overlay(model, key);
    }
    if model.editor.snippet_open {
        crate::screens::editor::handle_snippet_key(model, key);
        return Vec::new();
    }
    if model.editor.parameter_prompt {
        crate::screens::editor::handle_parameter_key(model, key);
        if !model.editor.parameter_prompt {
            return start_query(model);
        }
        return Vec::new();
    }
    if model.admin.open {
        return handle_admin_key(model, key);
    }
    if model.schema_editor.preview.is_some() {
        return match key.code {
            KeyCode::Esc => {
                model.schema_editor.preview = None;
                Vec::new()
            }
            KeyCode::Enter => apply_ddl(model),
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
            KeyCode::Char('l') if model.schema_diff.source_prompt => {
                if let Some(session) = model.active_session {
                    model.schema_diff.left = Some(dexo_app::schema_diff::DiffSource::Live(
                        session.0.to_string(),
                    ));
                    model.schema_diff.error = None;
                }
                Vec::new()
            }
            KeyCode::Char('r') if model.schema_diff.source_prompt => {
                if let Some(session) = model.active_session {
                    model.schema_diff.right = Some(dexo_app::schema_diff::DiffSource::Live(
                        session.0.to_string(),
                    ));
                    model.schema_diff.error = None;
                }
                Vec::new()
            }
            KeyCode::Enter if model.schema_diff.source_prompt => request_schema_diff(model),
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
    if model.security.open {
        return match key.code {
            KeyCode::Esc => {
                model.security.open = false;
                Vec::new()
            }
            KeyCode::Up => {
                model.security.select_previous();
                Vec::new()
            }
            KeyCode::Down => {
                model.security.select_next();
                Vec::new()
            }
            KeyCode::Enter => open_security_change_preview(model),
            _ => Vec::new(),
        };
    }
    if model.diagnostics.open {
        return match key.code {
            KeyCode::Esc => {
                model.diagnostics.open = false;
                model.diagnostics.writing = false;
                Vec::new()
            }
            KeyCode::Enter if !model.diagnostics.writing => open_diagnostics_picker(model),
            _ => Vec::new(),
        };
    }
    if model.transfer.open {
        return match key.code {
            KeyCode::Esc => {
                model.transfer.open = false;
                Vec::new()
            }
            KeyCode::Tab => {
                model.transfer.footer = model.transfer.footer.next();
                Vec::new()
            }
            KeyCode::BackTab => {
                model.transfer.footer = model.transfer.footer.prev();
                Vec::new()
            }
            KeyCode::Enter
                if model.transfer.footer == crate::widgets::form::FooterFocus::Cancel =>
            {
                model.transfer.open = false;
                Vec::new()
            }
            KeyCode::Enter => run_transfer(model),
            KeyCode::Backspace
                if model.transfer.footer == crate::widgets::form::FooterFocus::Input =>
            {
                model.transfer.path.pop();
                Vec::new()
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                open_file_picker(model, crate::screens::file_picker::FilePickerMode::Transfer);
                Vec::new()
            }
            KeyCode::Char(ch)
                if model.transfer.footer == crate::widgets::form::FooterFocus::Input
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                model.transfer.path.push(ch);
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
            KeyCode::Enter => update(model, Action::ApplyChanges),
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
                model.mcp_profiles.confirm_revoke = false;
                Vec::new()
            }
            KeyCode::Up => {
                model.mcp_profiles.select_previous();
                Vec::new()
            }
            KeyCode::Down => {
                model.mcp_profiles.select_next();
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Char('r') if model.mcp_profiles.confirm_revoke => {
                vec![Effect::RevokeAllMcpGrants]
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
                model.settings.confirm_reset = false;
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Char('r') => update(model, Action::ConfirmResetSettings),
            KeyCode::Char('t') => update(model, Action::CycleTheme),
            KeyCode::Char('k') => update(model, Action::CycleKeymap),
            KeyCode::Char('m') => update(model, Action::ToggleMouse),
            _ => Vec::new(),
        };
    }
    if model.recovery.open {
        return match key.code {
            KeyCode::Esc => {
                model.recovery.open = false;
                model.recovery.confirm_discard = false;
                Vec::new()
            }
            KeyCode::Enter if model.recovery.confirm_discard => {
                update(model, Action::ConfirmDiscardRecovery)
            }
            KeyCode::Enter | KeyCode::Char('y') => update(model, Action::ConfirmRecover),
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
            if let Some(invocation) = crate::palette::invocation_by_id(model, command) {
                return invoke_palette(model, invocation);
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
    if model.tabs.active == 0 && crate::screens::editor::handle_key(model, key) {
        crate::screens::editor::refresh_intelligence(model, false);
        return Vec::new();
    }
    if model.focus == Focus::Editor && model.tabs.active != 0 {
        return handle_editor_tab_key(model, key);
    }
    Vec::new()
}

fn handle_editor_tab_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let ddl_is_form = model.tabs.active == 2 && model.inspector.ddl.is_none();
    match key.code {
        KeyCode::Tab if model.tabs.active == 2 => {
            model.schema_editor.focus_next();
        }
        KeyCode::Up if ddl_is_form => {
            model.schema_editor.focus_prev();
        }
        KeyCode::Down if ddl_is_form => {
            model.schema_editor.focus_next();
        }
        KeyCode::Up => {
            model.tabs.scroll = model.tabs.scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            model.tabs.scroll = model.tabs.scroll.saturating_add(1);
        }
        _ => {}
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
        KeyCode::Enter | KeyCode::Char(' ') if model.connection_form.on_advanced() => {
            model.connection_form.toggle_advanced();
            Vec::new()
        }

        KeyCode::Left if model.connection_form.on_advanced() => {
            model.connection_form.set_advanced(false);
            Vec::new()
        }
        KeyCode::Right if model.connection_form.on_advanced() => {
            model.connection_form.set_advanced(true);
            Vec::new()
        }
        KeyCode::Esc => {
            model.connection_form.close();
            Vec::new()
        }
        KeyCode::Enter if model.connection_form.on_cancel() => {
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
        KeyCode::Left if model.connection_form.on_cancel() => {
            model.connection_form.focus_prev();
            Vec::new()
        }
        KeyCode::Right if model.connection_form.on_submit() => {
            model.connection_form.focus_next();
            Vec::new()
        }
        KeyCode::Left => {
            model.connection_form.cycle_driver(-1);
            Vec::new()
        }
        KeyCode::Right => {
            model.connection_form.cycle_driver(1);
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

fn handle_secret_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::Cancel,
            },
        ),
        KeyCode::Char('s') => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::SessionOnly,
            },
        ),
        KeyCode::Char('k') => update(
            model,
            Action::SubmitSecret {
                kind: crate::screens::secret_prompt::SecretChoiceKind::SaveToKeychain,
            },
        ),
        _ => Vec::new(),
    }
}

fn handle_connections_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if model.connections.delete_target.is_some() {
        return match key.code {
            KeyCode::Esc => {
                model.connections.delete_target = None;
                Vec::new()
            }
            KeyCode::Char('k') => update(
                model,
                Action::ConfirmDeleteProfile {
                    decision: crate::screens::secret_prompt::DeleteSecretDecision::KeepSecrets,
                },
            ),
            KeyCode::Char('d') => update(
                model,
                Action::ConfirmDeleteProfile {
                    decision: crate::screens::secret_prompt::DeleteSecretDecision::DeleteSecrets,
                },
            ),
            _ => Vec::new(),
        };
    }
    match key.code {
        KeyCode::Esc => {
            model.connections.open = false;
            model.connections.intent = None;
            model.connections.error = None;
            Vec::new()
        }
        KeyCode::Enter => choose_connection_intent(model),
        KeyCode::Up => {
            if model.connections.selected_profile > 0 {
                model.connections.selected_profile -= 1;
            }
            Vec::new()
        }
        KeyCode::Down => {
            if model.connections.selected_profile + 1 < model.connections.profiles.len() {
                model.connections.selected_profile += 1;
            }
            Vec::new()
        }
        KeyCode::Char('n') => update(model, Action::OpenConnectionForm),
        KeyCode::Char('e') => update(model, Action::EditSelectedConnection),
        KeyCode::Char('d') => update(model, Action::DuplicateConnection),
        KeyCode::Char('t') => update(model, Action::TestConnection),
        KeyCode::Char('x') => update(model, Action::DeleteConnection),
        KeyCode::Char('c') => update(model, Action::CloseSelectedSession),
        _ => Vec::new(),
    }
}

fn submit_secret(
    model: &mut Model,
    kind: crate::screens::secret_prompt::SecretChoiceKind,
) -> Vec<Effect> {
    let profile = model.secret_prompt.profile.clone();
    let secret = model.secret_prompt.buffer.clone();
    model.secret_prompt.close();
    match (kind, profile) {
        (crate::screens::secret_prompt::SecretChoiceKind::Cancel, _) => Vec::new(),
        (_, None) => Vec::new(),
        (kind, Some(profile)) => vec![Effect::SubmitSecret {
            kind,
            profile,
            secret,
        }],
    }
}

fn confirm_delete(
    model: &mut Model,
    decision: crate::screens::secret_prompt::DeleteSecretDecision,
) -> Vec<Effect> {
    let Some((profile, delete_secrets)) = model.connections.delete_decision(decision) else {
        return Vec::new();
    };
    model.connections.delete_target = None;
    vec![Effect::DeleteProfile {
        profile,
        delete_secrets,
    }]
}

fn explorer_visible_rows(model: &Model) -> usize {
    let area = Rect::new(0, 0, model.width.max(1), model.height.max(1));
    let plan = LayoutPlan::for_area_with(area, Some(&model.panes));
    let height = if matches!(plan.mode, crate::layout::LayoutMode::Compact) {
        plan.content.height
    } else {
        plan.explorer.height
    };
    height
        .saturating_sub(2)
        .saturating_sub((2 + model.connections.profiles.len()) as u16)
        .max(1) as usize
}

fn connect_selected(model: &mut Model) -> Vec<Effect> {
    let Some(profile) = model.connections.selected().cloned() else {
        return Vec::new();
    };
    if let Some(session) = model.connections.session_for(&profile.name).cloned() {
        return activate_existing_session(model, &profile, session);
    }
    model.connect_token = model.connect_token.saturating_add(1);
    model.connections.pending_connect = Some(model.connect_token);
    vec![Effect::ConnectProfile {
        profile,
        token: model.connect_token,
    }]
}

fn activate_sidebar_connection(model: &mut Model) -> Vec<Effect> {
    if model.explorer.connection_cursor >= model.connections.profiles.len() {
        return Vec::new();
    }
    model.connections.selected_profile = model.explorer.connection_cursor;
    connect_selected(model)
}

fn move_sidebar_selection(model: &mut Model, delta: i32) {
    use crate::screens::explorer::SidebarFocus;

    match (model.explorer.sidebar_focus, delta) {
        (SidebarFocus::Connections, -1) if model.explorer.connection_cursor > 0 => {
            model.explorer.connection_cursor -= 1;
        }
        (SidebarFocus::Connections, -1) => {}
        (SidebarFocus::Connections, 1)
            if model.explorer.connection_cursor + 1 < model.connections.profiles.len() =>
        {
            model.explorer.connection_cursor += 1;
        }
        (SidebarFocus::Connections, 1) => {
            model.explorer.sidebar_focus = SidebarFocus::Catalog;
        }
        (SidebarFocus::Catalog, -1)
            if model.explorer.selected_index() == 0 && !model.connections.profiles.is_empty() =>
        {
            model.explorer.sidebar_focus = SidebarFocus::Connections;
            model.explorer.connection_cursor = model.connections.profiles.len() - 1;
        }
        (SidebarFocus::Catalog, _) => {
            model.explorer.move_selection(delta);
            model.explorer.sync_scroll(explorer_visible_rows(model));
        }
        _ => {}
    }
}


fn enter_offline_explorer(model: &mut Model) -> Vec<Effect> {
    model.explorer.clear();
    model.explorer.offline = true;
    if model.connection.name.is_empty() {
        return Vec::new();
    }
    vec![Effect::LoadOfflineCatalog {
        connection_id: model.connection.name.clone(),
        database_name: catalog_database(model),
        generation: model.session_generation,
    }]
}

fn activate_existing_session(
    model: &mut Model,
    profile: &dexo_app::ConnectionProfile,
    session: crate::screens::connections::SessionRow,
) -> Vec<Effect> {
    model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
    model.focus = Focus::Editor;
    if model.active_session == Some(session.id) {
        return Vec::new();
    }
    model.connection.name = profile.name.clone();
    model.connection.ready = true;
    model.explorer.offline = false;
    model.connection.environment = session.environment.clone();
    model.connection.read_only = session.read_only;
    model.connection.driver = session.driver.clone();
    model.active_session = Some(session.id);
    model.session_generation = session.generation;
    model.connections.selected_session = Some(session.id);
    model.transaction = session.transaction;
    model.explorer.clear();
    let operation = crate::runtime::OperationId::new();
    vec![Effect::LoadCatalogChildren {
        parent: None,
        operation,
        session: session.id,
        generation: session.generation,
        replace_roots: true,
        include_system: model.explorer.include_system,
    }]
}

fn test_connection(model: &mut Model) -> Vec<Effect> {
    if model.connection_form.open {
        return match model.connection_form.submit() {
            Some((input, password)) => vec![Effect::TestConnection { input, password }],
            None => Vec::new(),
        };
    }
    model
        .connections
        .selected()
        .cloned()
        .map(|profile| Effect::TestSavedProfile { profile })
        .into_iter()
        .collect()
}

fn save_connection(model: &mut Model) -> Vec<Effect> {
    match model.connection_form.submit() {
        Some((input, password)) => {
            if let Some(original) = model.connection_form.editing.clone() {
                match dexo_app::test_connection_input(input) {
                    Ok(mut profile) => {
                        profile.id = original.id;
                        profile.secret_ref = original.secret_ref;
                        profile.secret_refs = original.secret_refs;
                        profile.project_id = original.project_id;
                        model.connection_form.close();
                        vec![Effect::SaveProfile { profile }]
                    }
                    Err(error) => {
                        model.connection_form.set_error(error.to_string());
                        Vec::new()
                    }
                }
            } else {
                vec![Effect::CreateConnection { input, password }]
            }
        }
        None => Vec::new(),
    }
}

fn handle_transaction_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.transaction_prompt.open = false;
            model.transaction_prompt.error = None;
            Vec::new()
        }
        KeyCode::Tab => {
            model.transaction_prompt.footer = model.transaction_prompt.footer.next();
            Vec::new()
        }
        KeyCode::BackTab => {
            model.transaction_prompt.footer = model.transaction_prompt.footer.prev();
            Vec::new()
        }
        KeyCode::Enter
            if model.transaction_prompt.footer == crate::widgets::form::FooterFocus::Cancel =>
        {
            model.transaction_prompt.open = false;
            model.transaction_prompt.error = None;
            Vec::new()
        }
        KeyCode::Enter => submit_savepoint_prompt(model),
        KeyCode::Backspace
            if model.transaction_prompt.footer == crate::widgets::form::FooterFocus::Input =>
        {
            model.transaction_prompt.name.pop();
            Vec::new()
        }
        KeyCode::Char(ch)
            if model.transaction_prompt.footer == crate::widgets::form::FooterFocus::Input
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            model.transaction_prompt.name.push(ch);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_data_query_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.data.query_prompt.open = false;
            model.data.query_prompt.error = None;
            Vec::new()
        }
        KeyCode::Enter
            if model.data.query_prompt.footer == crate::widgets::form::FooterFocus::Cancel =>
        {
            model.data.query_prompt.open = false;
            model.data.query_prompt.error = None;
            Vec::new()
        }
        KeyCode::Enter => submit_data_query_prompt(model),
        KeyCode::Down => {
            model.data.query_prompt.footer = model.data.query_prompt.footer.next();
            Vec::new()
        }
        KeyCode::Up => {
            model.data.query_prompt.footer = model.data.query_prompt.footer.prev();
            Vec::new()
        }
        KeyCode::Tab => {
            match model.data.query_prompt.intent {
                Some(crate::screens::data::DataQueryIntent::Sort) => {
                    model.data.query_prompt.descending = !model.data.query_prompt.descending;
                }
                Some(crate::screens::data::DataQueryIntent::Filter) => {
                    model.data.query_prompt.focus_value = !model.data.query_prompt.focus_value;
                }
                None => {}
            }
            Vec::new()
        }
        KeyCode::Backspace
            if model.data.query_prompt.footer == crate::widgets::form::FooterFocus::Input =>
        {
            if model.data.query_prompt.focus_value {
                model.data.query_prompt.value.pop();
            } else {
                model.data.query_prompt.column.pop();
            }
            Vec::new()
        }
        KeyCode::Char(ch)
            if model.data.query_prompt.footer == crate::widgets::form::FooterFocus::Input
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            if model.data.query_prompt.focus_value {
                model.data.query_prompt.value.push(ch);
            } else {
                model.data.query_prompt.column.push(ch);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn open_palette(model: &mut Model) {
    if !model.palette.open {
        model.palette.origin_focus = Some(model.focus);
    }
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
            model.focus = model.palette.origin_focus.take().unwrap_or(Focus::Editor);
        }
    }
}

fn toggle_help(model: &mut Model) {
    if model.help.open {
        model.help.open = false;
        model.help.scroll = 0;
        return;
    }
    close_palette(model);
    model.results_menu.open = false;
    model.help.open = true;
    model.help.scroll = 0;
}

fn handle_help_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::F(1) => {
            model.help.open = false;
            model.help.scroll = 0;
            Vec::new()
        }
        KeyCode::Char('?') => {
            model.help.open = false;
            model.help.scroll = 0;
            Vec::new()
        }
        KeyCode::Up | KeyCode::PageUp => {
            model.help.scroll = model.help.scroll.saturating_sub(1);
            Vec::new()
        }
        KeyCode::Down | KeyCode::PageDown => {
            model.help.scroll = model.help.scroll.saturating_add(1);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn open_results_menu(model: &mut Model) {
    model.results.ensure_cursor();
    if model.results.row_count() == 0 {
        return;
    }
    model.results_menu.open = true;
    model.results_menu.selected = 0;
    model.results_menu.offset = 0;
}

fn handle_results_menu_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let count = crate::palette::results_menu_items().len();
    match key.code {
        KeyCode::Esc => {
            model.results_menu.open = false;
            Vec::new()
        }
        KeyCode::Up => {
            if count > 0 {
                model.results_menu.selected = model.results_menu.selected.saturating_sub(1);
            }
            Vec::new()
        }
        KeyCode::Down => {
            if count > 0 {
                model.results_menu.selected =
                    (model.results_menu.selected + 1).min(count.saturating_sub(1));
            }
            Vec::new()
        }
        KeyCode::Enter => pick_results_menu(model),
        _ => Vec::new(),
    }
}

fn pick_results_menu(model: &mut Model) -> Vec<Effect> {
    let items = crate::palette::results_menu_items();
    let Some((id, _)) = items.get(model.results_menu.selected) else {
        model.results_menu.open = false;
        return Vec::new();
    };
    model.results_menu.open = false;
    match *id {
        "copy-cell" => {
            if let Some((row, col)) = model.results.selection() {
                model.results.select_cell(row, col);
            }
            copy_grid(model, dexo_app::data::CopyFormat::Text)
        }
        other => {
            if other.starts_with("data.copy") && model.results.picked_rows.is_empty() {
                match model.results.kind {
                    crate::model::GridSelection::Range { start, end } => {
                        let last_col = model.results.columns().len().saturating_sub(1);
                        model.results.select_range((start.0, 0), (end.0, last_col));
                    }
                    _ => {
                        if let Some((row, _)) = model.results.selection() {
                            model.results.select_row(row);
                        }
                    }
                }
            }
            if let Some(invocation) = crate::palette::invocation_by_id(model, other) {
                invoke_palette(model, invocation)
            } else {
                Vec::new()
            }
        }
    }
}

fn click_results_row(model: &mut Model, row: usize, extend: bool) {
    model.results.ensure_cursor();
    let col = model.results.selection().map(|(_, col)| col).unwrap_or(0);
    let last = model.results.row_count().saturating_sub(1);
    if model.results.row_count() == 0 {
        return;
    }
    let row = row.min(last);
    if extend {
        let start = match model.results.kind {
            crate::model::GridSelection::Range { start, .. } => start,
            _ => model.results.selection().unwrap_or((row, col)),
        };
        model.results.select_range(start, (row, col));
    } else {
        model.results.select_cell(row, col);
    }
}

fn apply_layout_preset(model: &mut Model, preset: crate::layout::LayoutPreset) {
    model.layout_preset = preset;
    model.panes = preset.apply(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn adjust_results_height(model: &mut Model, delta: i16) {
    let next = (model.panes.results_height as i16 + delta).max(3) as u16;
    model.panes.results_visible = true;
    model.panes.results_height = next;
    model.panes = model.panes.clamp(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn adjust_explorer_width(model: &mut Model, delta: i16) {
    let next = (model.panes.explorer_width as i16 + delta).max(8) as u16;
    model.panes.explorer_visible = true;
    model.panes.explorer_width = next;
    model.panes = model.panes.clamp(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn adjust_inspector_width(model: &mut Model, delta: i16) {
    let next = (model.panes.inspector_width as i16 + delta).max(8) as u16;
    model.panes.inspector_visible = true;
    model.panes.inspector_width = next;
    model.panes = model.panes.clamp(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn active_connection_uuid(model: &Model) -> Option<String> {
    let name = model.connection.name.as_str();
    if name.is_empty() {
        return None;
    }
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| row.profile.id.0.to_string())
}

fn persist_layout_effect(model: &Model) -> Effect {
    Effect::PersistLayout {
        project_id: model.project_id.clone(),
        layout: model.workbench_layout(),
    }
}

fn checkpoint_dirty(model: &Model) -> Vec<Effect> {
    model
        .documents
        .iter()
        .filter(|document| document.is_dirty())
        .map(|document| match &document.path {
            Some(path) => Effect::AutosaveDocument {
                id: document.id.clone(),
                path: path.clone(),
                content: document.text(),
                revision: document.sql.revision(),
            },
            None => Effect::CheckpointRecovery(crate::action::RecoveryCheckpointRequest {
                document: document.id.clone(),
                project_id: model.project_id.clone(),
                title: document.title.clone(),
                content: document.text(),
            }),
        })
        .collect()
}

fn persist_history_effect(model: &Model) -> Vec<Effect> {
    let sql = model.active_document().text();
    if sql.trim().is_empty() {
        return Vec::new();
    }
    let entry = dexo_sql::HistoryEntry {
        sql,
        parameters: None,
    }
    .for_storage(model.editor.history_policy);
    // Sensitive parameter values are never stored; HistoryPolicy::SqlOnly is the default.
    vec![Effect::PersistHistory(
        crate::action::PersistHistoryRequest {
            project_id: if model.project_id.is_empty() {
                None
            } else {
                Some(model.project_id.clone())
            },
            connection_id: if model.connection.name.is_empty() {
                None
            } else {
                Some(model.connection.name.clone())
            },
            sql: entry.sql,
        },
    )]
}

fn start_query(model: &mut Model) -> Vec<Effect> {
    crate::screens::editor::end_typing(model);
    if model.active_document().text().trim().is_empty() {
        return Vec::new();
    }
    if !model.editor.parameters.is_empty()
        && model
            .editor
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.value, DbValue::Null))
    {
        model.editor.parameter_prompt = true;
        return Vec::new();
    }
    let statements = crate::screens::workbench::planned_statements(model);
    if statements.is_empty() {
        return Vec::new();
    }
    let operation = crate::runtime::OperationId::new();
    let session = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    let document = model.active_document().id.clone();
    let key = crate::runtime::OperationKey::new(
        operation,
        session.clone(),
        document.clone(),
        model.session_generation.max(1),
    );
    model.results.tabs = statements
        .iter()
        .enumerate()
        .map(|(index, sql)| {
            let mut tab = crate::model::ResultTab::new(
                crate::model::ResultKey {
                    operation: key.clone(),
                    index,
                },
                format!("result {}", index + 1),
            );
            tab.source_sql = Some(sql.clone());
            tab
        })
        .collect();
    model.results.active = 0;
    let request = QueryRequest::read(statements[0].clone(), 10_000);
    model.active_query = Some(request.id);
    model.active_operation = Some(operation);
    let mut effects = checkpoint_dirty(model);
    effects.push(Effect::StartScript(crate::action::ScriptRequest {
        key,
        statements,
        policy: model.script_policy,
        parameters: model
            .editor
            .parameters
            .iter()
            .map(|parameter| parameter.value.clone())
            .collect(),
        timeout: std::time::Duration::from_secs(30),
    }));
    effects
}

fn cancel_query(model: &mut Model) -> Vec<Effect> {
    model
        .active_operation
        .map(Effect::CancelOperation)
        .into_iter()
        .collect()
}

fn apply_bootstrap(model: &mut Model, state: crate::runtime::storage_worker::BootstrapState) {
    model.project = state.active_project.name.clone();
    model.project_id = state.active_project.id.0.to_string();
    model.projects.load(state.projects);
    model.projects.touch_recent(&state.active_project.name);
    if !state.documents.is_empty() {
        model.documents = state
            .documents
            .into_iter()
            .map(document_from_stored)
            .collect();
        model.active_document = 0;
    }
    apply_layout(model, state.layout);
    if state.recovery.needs_recovery() {
        model.recovery.open = true;
        model.recovery.transaction = state.recovery.transaction;
        model.recovery.checkpoints = state
            .recovery
            .documents
            .iter()
            .map(|document| {
                (
                    document.id.clone(),
                    document.title.clone(),
                    document.content.clone(),
                )
            })
            .collect();
        model.recovery.documents = state
            .recovery
            .documents
            .into_iter()
            .map(|document| document.title)
            .collect();
    }
    model.connections.load_profiles(state.connections);
    model.editor.snippets = state.snippets;
    apply_saved_settings(model);
}

fn document_from_stored(stored: dexo_storage::StoredDocument) -> crate::model::EditorDocument {
    let mut document = crate::model::EditorDocument::with_text(&stored.content);
    document.id = stored.id;
    document.title = stored.title;
    document.path = stored.path.map(std::path::PathBuf::from);
    document
}

fn apply_layout(model: &mut Model, layout: Option<dexo_storage::WorkbenchLayout>) {
    let Some(layout) = layout.map(|layout| layout.clamp(model.width, model.height)) else {
        return;
    };
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
    if let Some(id) = &layout.active_document_id
        && let Some(index) = model
            .documents
            .iter()
            .position(|document| &document.id == id)
    {
        model.active_document = index;
    }
    if let Some(name) = layout.active_connection_id {
        model.connection.name = name;
    }
}

fn ensure_result_tab<'a>(
    model: &'a mut Model,
    key: &crate::runtime::OperationKey,
    index: usize,
) -> &'a mut crate::model::ResultTab {
    while model.results.tabs.len() <= index {
        let next = model.results.tabs.len();
        let mut tab = crate::model::ResultTab::new(
            crate::model::ResultKey {
                operation: key.clone(),
                index: next,
            },
            format!("result {}", next + 1),
        );
        tab.status = crate::model::OperationStatus::Running;
        model.results.tabs.push(tab);
    }
    let tab = &mut model.results.tabs[index];
    tab.key = crate::model::ResultKey {
        operation: key.clone(),
        index,
    };
    tab.status = crate::model::OperationStatus::Running;
    tab
}

fn result_tab_mut<'a>(
    model: &'a mut Model,
    key: &crate::runtime::OperationKey,
    index: usize,
) -> Option<&'a mut crate::model::ResultTab> {
    if !operation_matches(model, key) {
        return None;
    }
    model.results.tabs.get_mut(index)
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
    let document = model.active_document().id.as_str();
    key.belongs_to(&session, document, generation)
}

fn catalog_generation_matches(model: &Model, session: &str, generation: u64) -> bool {
    let current = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    session == current && generation == model.session_generation
}

fn catalog_database(model: &Model) -> String {
    if model.schema.is_empty() {
        model.connection.name.clone()
    } else {
        model.schema.clone()
    }
}

fn catalog_followup_effects(model: &Model, capture: bool) -> Vec<Effect> {
    let mut effects = Vec::new();
    if capture && let Some(session) = model.active_session {
        effects.push(Effect::CaptureCatalogSnapshot {
            connection_id: model.connection.name.clone(),
            database_name: catalog_database(model),
            session,
            include_system: model.explorer.include_system,
        });
    }
    if !model.project_id.is_empty() && !model.connection.name.is_empty() {
        effects.push(Effect::LoadObjectUsage {
            project_id: model.project_id.clone(),
            connection_id: model.connection.name.clone(),
        });
    }
    effects
}

fn toggle_favorite(model: &mut Model) -> Vec<Effect> {
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    model.explorer.toggle_favorite(&id);
    let favorite = model
        .explorer
        .selected_node()
        .map(|node| node.favorite)
        .unwrap_or(false);
    if model.project_id.is_empty() || model.connection.name.is_empty() {
        return Vec::new();
    }
    vec![Effect::PersistFavorite {
        project_id: model.project_id.clone(),
        connection_id: model.connection.name.clone(),
        object_id: id.as_str().to_string(),
        favorite,
    }]
}

fn catalog_load_effect(
    model: &Model,
    parent: Option<dexo_driver_api::ObjectId>,
    operation: crate::runtime::OperationId,
    replace_roots: bool,
) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    vec![Effect::LoadCatalogChildren {
        parent,
        operation,
        session,
        generation: model.session_generation,
        replace_roots,
        include_system: model.explorer.include_system,
    }]
}

fn expand_or_open_selected(model: &mut Model) -> Vec<Effect> {
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    if model
        .explorer
        .selected_node()
        .is_some_and(|node| node.expanded)
    {
        model.explorer.collapse(&id);
        return Vec::new();
    }
    if model
        .explorer
        .selected_node()
        .is_some_and(|node| crate::screens::explorer::opens_table_data(&node.kind))
    {
        return open_selected_table(model);
    }
    expand_selected_catalog(model)
}

fn expand_selected_catalog(model: &mut Model) -> Vec<Effect> {
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    let operation = crate::runtime::OperationId::new();
    if model.explorer.expand_with(&id, operation) {
        catalog_load_effect(model, Some(id), operation, false)
    } else {
        Vec::new()
    }
}

fn open_selected_table(model: &mut Model) -> Vec<Effect> {
    let mut effects = open_object_data(model);
    effects.extend(open_inspector(model));
    if let Some(id) = model.explorer.selected.clone() {
        let operation = crate::runtime::OperationId::new();
        if model.explorer.expand_with(&id, operation) {
            effects.extend(catalog_load_effect(model, Some(id), operation, false));
        }
    }
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::LoadTableData { .. }))
    {
        model.focus = Focus::Results;
        model.panes.results_visible = true;
        model.panes.inspector_visible = true;
    }
    effects
}

fn refresh_catalog(model: &mut Model, all: bool) -> Vec<Effect> {
    if model.active_session.is_none() {
        model
            .messages
            .push("connect a session to refresh the catalog".into());
        return Vec::new();
    }
    let operation = crate::runtime::OperationId::new();
    if all {
        model.explorer.clear();
        return catalog_load_effect(model, None, operation, true);
    }
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    model.explorer.expand_with(&id, operation);
    catalog_load_effect(model, Some(id), operation, false)
}

fn open_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    if model.active_session.is_none() {
        model
            .messages
            .push("connect a session to browse table data".into());
        return Vec::new();
    }
    model.data.target = dexo_app::parse_qualified(&node.qualified);
    model.data.loading = true;
    model.data.last_error = None;
    model.data.page_offset = 0;
    reload_object_data(model)
}

fn change_data_page(model: &mut Model, offset: u64) -> Vec<Effect> {
    if model.active_session.is_none() {
        model.data.last_error = Some("connect a session first".into());
        return Vec::new();
    }
    if model.data.target.object().is_empty() {
        model.data.last_error = Some("open a table first".into());
        return Vec::new();
    }
    model.data.page_offset = offset;
    model.data.loading = true;
    let effects = reload_object_data(model);
    if effects.is_empty() {
        model.data.loading = false;
    }
    effects
}

fn apply_remote_query(model: &mut Model) -> Vec<Effect> {
    let source = model
        .results
        .tabs
        .get(model.results.active)
        .and_then(|tab| tab.source_sql.clone());
    match source {
        Some(sql) => rerun_derived(model, sql),
        None => reload_object_data(model),
    }
}

fn rerun_derived(model: &mut Model, sql: String) -> Vec<Effect> {
    let page = match dexo_driver_api::Page::new(model.data.page_offset, model.data.page_limit) {
        Ok(page) => page,
        Err(error) => {
            model.messages.push(error.to_string());
            return Vec::new();
        }
    };
    match dexo_sql::derive_page(&sql, &model.data.sort, &model.data.filter, page) {
        Ok(derived) => {
            if let Some(tab) = model.results.tabs.get_mut(model.results.active) {
                tab.local_only = None;
            }
            let mut parameters = Vec::new();
            if let Some(filter) = &model.data.filter {
                parameters = dexo_sql::filter_values(filter);
            }
            let derived = match model.data.dialect {
                dexo_app::data::SqlDialect::Postgres => postgres_placeholders(&derived),
                dexo_app::data::SqlDialect::Mysql => derived,
            };
            start_derived_script(model, derived, parameters)
        }
        Err(reason) => {
            if let Some(tab) = model.results.tabs.get_mut(model.results.active) {
                tab.local_only = Some(reason.clone());
            }
            model.messages.push(format!("local-only: {reason}"));
            Vec::new()
        }
    }
}

fn postgres_placeholders(sql: &str) -> String {
    // ponytail: rewrite `?` left-to-right; ceiling: `?` inside string literals.
    let mut n = 0;
    let mut out = String::with_capacity(sql.len());
    for ch in sql.chars() {
        if ch == '?' {
            n += 1;
            out.push_str(&format!("${n}"));
        } else {
            out.push(ch);
        }
    }
    out
}

fn start_derived_script(model: &mut Model, sql: String, parameters: Vec<DbValue>) -> Vec<Effect> {
    let operation = crate::runtime::OperationId::new();
    let session = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    let document = model.active_document().id.clone();
    let key = crate::runtime::OperationKey::new(
        operation,
        session,
        document,
        model.session_generation.max(1),
    );
    let source_sql = model
        .results
        .tabs
        .get(model.results.active)
        .and_then(|tab| tab.source_sql.clone());
    let mut tab = crate::model::ResultTab::new(
        crate::model::ResultKey {
            operation: key.clone(),
            index: 0,
        },
        "result 1",
    );
    tab.source_sql = source_sql;
    tab.status = crate::model::OperationStatus::Running;
    model.results.tabs = vec![tab];
    model.results.active = 0;
    model.active_operation = Some(operation);
    vec![Effect::StartScript(crate::action::ScriptRequest {
        key,
        statements: vec![sql],
        policy: model.script_policy,
        parameters,
        timeout: std::time::Duration::from_secs(30),
    })]
}

fn reload_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    match crate::runtime::data_manager::table_request(
        model.data.target.clone(),
        Vec::new(),
        model.data.filter.clone(),
        model.data.sort.clone(),
        model.data.page_offset,
        model.data.page_limit,
    ) {
        Ok(request) => vec![Effect::LoadTableData {
            request,
            session,
            generation: model.session_generation,
        }],
        Err(message) => {
            model.messages.push(message);
            Vec::new()
        }
    }
}

fn open_inspector(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    model.inspector =
        crate::screens::object_inspector::ObjectInspector::open_loading(&node.qualified);
    vec![Effect::LoadObjectInspector {
        id: node.id.clone(),
        session,
        generation: model.session_generation,
    }]
}

fn goto_definition(model: &mut Model) -> Vec<Effect> {
    let sql = model.active_document().text();
    let cursor = model.active_document().cursor();
    let catalog = dexo_app::SnapshotCatalog::new(flatten_explorer(&model.explorer));
    let Some(target) = dexo_sql::definition_at(&sql, cursor, &catalog) else {
        model.messages.push("no definition at cursor".into());
        return Vec::new();
    };
    let wanted = target.display_unquoted();
    if let Some(id) = find_qualified(&model.explorer, &wanted) {
        model.explorer.select(id.clone());
        let operation = crate::runtime::OperationId::new();
        if model.explorer.expand_with(&id, operation) {
            return catalog_load_effect(model, Some(id), operation, false);
        }
    }
    Vec::new()
}

fn flatten_explorer(
    explorer: &crate::screens::explorer::ExplorerState,
) -> Vec<dexo_driver_api::CatalogObject> {
    explorer.flatten()
}

fn find_qualified(
    explorer: &crate::screens::explorer::ExplorerState,
    qualified: &str,
) -> Option<dexo_driver_api::ObjectId> {
    fn walk(
        nodes: &[crate::screens::explorer::ExplorerNode],
        qualified: &str,
    ) -> Option<dexo_driver_api::ObjectId> {
        for node in nodes {
            if node.qualified == qualified
                || qualified.starts_with(&format!("{}.", node.qualified))
                || node.qualified.ends_with(&format!(".{qualified}"))
                || qualified.ends_with(&format!(".{}", node.label))
            {
                return Some(node.id.clone());
            }
            if let Some(found) = walk(&node.children, qualified) {
                return Some(found);
            }
        }
        None
    }
    walk(&explorer.roots, qualified)
}

fn copy_selected(model: &mut Model, qualified: bool) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    let text = if qualified {
        node.qualified.clone()
    } else {
        node.label.clone()
    };
    vec![Effect::CopyToClipboard { text }]
}

fn copy_ddl(model: &mut Model) -> Vec<Effect> {
    match &model.inspector.ddl {
        Some(sql) => vec![Effect::CopyToClipboard { text: sql.clone() }],
        None => {
            model.messages.push("DDL is not loaded".into());
            Vec::new()
        }
    }
}

fn inspect_selected(model: &mut Model) -> Vec<Effect> {
    let Some((row, col)) = model.results.selection() else {
        return Vec::new();
    };
    if let Some(cell) = model.results.cell_at(row, col).cloned() {
        match cell {
            crate::model::GridCell::Remote(value) => {
                let Some(session) = model.active_session else {
                    model
                        .messages
                        .push("connect a session to fetch the value".into());
                    return Vec::new();
                };
                return vec![Effect::FetchValue {
                    value,
                    offset: 0,
                    limit: 64 * 1024,
                    session,
                    generation: model.session_generation,
                }];
            }
            crate::model::GridCell::Spool { path, total, .. } => {
                let bytes = std::fs::read(&path).unwrap_or_default();
                let loaded = bytes.len() as u64;
                model.data.viewer = Some(inspect_value(
                    &DbValue::Bytes(bytes),
                    loaded.min(total),
                    total,
                ));
                return Vec::new();
            }
            crate::model::GridCell::Inline(value) => {
                model.data.viewer = Some(crate::screens::value_viewer::view(&value));
                return Vec::new();
            }
        }
    }
    let Some(value) = model
        .results
        .rows()
        .get(row)
        .and_then(|cells| cells.get(col))
    else {
        return Vec::new();
    };
    model.data.viewer = Some(crate::screens::value_viewer::view(value));
    Vec::new()
}

fn promote_remote_cells(model: &mut Model, columns: &[dexo_driver_api::ColumnMeta]) {
    let Some(identity_cols) = dexo_app::data::RowIdentity::from_table(&model.data.table) else {
        return;
    };
    let rows = model.results.rows_snapshot();
    for (row_idx, row) in rows.iter().enumerate() {
        let identity: Vec<(dexo_driver_api::ColumnId, DbValue)> = identity_cols
            .iter()
            .filter_map(|name| {
                let index = columns.iter().position(|column| column.name == *name)?;
                Some((
                    dexo_driver_api::ColumnId(name.clone()),
                    row.get(index).cloned().unwrap_or(DbValue::Null),
                ))
            })
            .collect();
        if identity.len() != identity_cols.len() {
            continue;
        }
        for (col_idx, value) in row.iter().enumerate() {
            let total = match value {
                DbValue::Native {
                    type_name, text, ..
                } if type_name.starts_with("truncated") => text.parse().unwrap_or(0),
                _ => continue,
            };
            if total == 0 {
                continue;
            }
            let Some(column) = columns.get(col_idx) else {
                continue;
            };
            model.results.cells.insert(
                (row_idx, col_idx),
                crate::model::GridCell::Remote(dexo_driver_api::RemoteValueRef {
                    object: model.data.target.clone(),
                    identity: identity.clone(),
                    column: dexo_driver_api::ColumnId(column.name.clone()),
                    total,
                }),
            );
        }
    }
}

fn open_related(model: &mut Model) -> Vec<Effect> {
    let Some(fk) = model.data.related_fk.clone() else {
        model.messages.push("no related foreign key".into());
        return Vec::new();
    };
    let Some(filter) = related_filter(&fk, &model.data.related_row) else {
        model
            .messages
            .push("foreign key is null; navigation disabled".into());
        return Vec::new();
    };
    let title = fk.referenced_table.display_unquoted();
    model.data.crumbs.push((
        model.data.target.clone(),
        model.data.filter.clone(),
        model.data.page_offset,
    ));
    model.data.crumb_forward.clear();
    model.tabs.titles.push(title.clone());
    model.tabs.active = model.tabs.titles.len() - 1;
    model.data.target = fk.referenced_table.clone();
    model.data.filter = Some(filter);
    model.data.related_open.push(title);
    model.data.page_offset = 0;
    model.data.loading = true;
    reload_object_data(model)
}

fn data_nav_back(model: &mut Model) -> Vec<Effect> {
    let Some((target, filter, offset)) = model.data.crumbs.pop() else {
        return Vec::new();
    };
    model.data.crumb_forward.push((
        model.data.target.clone(),
        model.data.filter.clone(),
        model.data.page_offset,
    ));
    model.data.related_open.pop();
    model.data.target = target;
    model.data.filter = filter;
    model.data.page_offset = offset;
    model.data.loading = true;
    reload_object_data(model)
}

fn copy_grid(model: &mut Model, format: dexo_app::data::CopyFormat) -> Vec<Effect> {
    match model.results.copy(format, model.data.dialect) {
        Ok(text) if text.len() > 8 * 1024 * 1024 => {
            model
                .messages
                .push("selection too large for clipboard; export to a file".into());
            Vec::new()
        }
        Ok(text) => vec![Effect::CopyToClipboard { text }],
        Err(message) => {
            model.messages.push(message);
            Vec::new()
        }
    }
}

fn apply_changes(model: &mut Model) -> Vec<Effect> {
    if model.connection.read_only {
        model.messages.push("connection is read-only".into());
        return Vec::new();
    }
    if let Some(review) = &model.data.review
        && review.production
        && !review.confirmed
    {
        model
            .messages
            .push("type the target to confirm production apply".into());
        return Vec::new();
    }
    let Some(session) = model.active_session else {
        model
            .messages
            .push("connect a session to apply changes".into());
        return Vec::new();
    };
    match dexo_app::data::mutations_for(model.data.target.clone(), &model.data.changes) {
        Ok(mutations) if mutations.is_empty() => Vec::new(),
        Ok(mutations) => vec![Effect::ApplyMutations {
            mutations,
            session,
            generation: model.session_generation,
        }],
        Err(error) => {
            model.messages.push(error.to_string());
            Vec::new()
        }
    }
}

fn open_ddl_preview(model: &mut Model) -> Vec<Effect> {
    if !model.schema_editor.validate() {
        return Vec::new();
    }
    let Ok(change) = model.schema_editor.to_change() else {
        return Vec::new();
    };
    let Some(session) = model.active_session else {
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
        let mut plan = dexo_driver_api::DdlPlan {
            transactional: true,
            ..dexo_driver_api::DdlPlan::default()
        };
        plan.push(sql, false);
        let preview = dexo_app::schema::preview_change(
            &change,
            plan,
            Vec::new(),
            Vec::new(),
            &dexo_app::schema::production_policy(),
        );
        model.schema_editor.open_preview(preview);
        return Vec::new();
    };
    vec![Effect::PreviewDdl {
        change,
        session,
        generation: model.session_generation,
    }]
}

fn apply_ddl(model: &mut Model) -> Vec<Effect> {
    let Some(preview) = &model.schema_editor.preview else {
        return Vec::new();
    };
    if matches!(
        preview.confirmation,
        dexo_app::schema::Confirmation::TypeTarget(_)
    ) && !preview.confirmed
    {
        return Vec::new();
    }
    let typed = preview.typed.clone();
    let Ok(change) = model.schema_editor.to_change() else {
        return Vec::new();
    };
    let Some(session) = model.active_session else {
        model.messages.push("ddl queued".into());
        model.schema_editor.preview = None;
        return Vec::new();
    };
    model.schema_editor.preview = None;
    vec![Effect::ApplyDdlChange {
        change,
        typed,
        session,
        generation: model.session_generation,
    }]
}

enum SavepointOp {
    Create,
    Rollback,
    Release,
}

fn savepoint_named(model: &Model, op: SavepointOp, name: String) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    match (op, model.transaction) {
        (SavepointOp::Create, TransactionState::Active) => {
            vec![Effect::Savepoint { session, name }]
        }
        (SavepointOp::Release, TransactionState::Active) => {
            vec![Effect::ReleaseSavepoint { session, name }]
        }
        (SavepointOp::Rollback, TransactionState::Active | TransactionState::Failed) => {
            vec![Effect::RollbackToSavepoint { session, name }]
        }
        _ => Vec::new(),
    }
}

fn open_savepoint_prompt(
    model: &mut Model,
    intent: crate::screens::transaction_prompt::SavepointIntent,
) -> Vec<Effect> {
    model.transaction_prompt.open = true;
    model.transaction_prompt.intent = Some(intent);
    model.transaction_prompt.name.clear();
    model.transaction_prompt.error = None;
    Vec::new()
}

fn submit_savepoint_prompt(model: &mut Model) -> Vec<Effect> {
    let name = model.transaction_prompt.name.trim();
    if name.is_empty() {
        model.transaction_prompt.error = Some("savepoint name is required".into());
        return Vec::new();
    }
    let name = name.to_string();
    let op = match model.transaction_prompt.intent {
        Some(crate::screens::transaction_prompt::SavepointIntent::Create) => SavepointOp::Create,
        Some(crate::screens::transaction_prompt::SavepointIntent::Rollback) => {
            SavepointOp::Rollback
        }
        Some(crate::screens::transaction_prompt::SavepointIntent::Release) => SavepointOp::Release,
        None => return Vec::new(),
    };
    let effects = savepoint_named(model, op, name);
    if effects.is_empty() {
        model.transaction_prompt.error = Some("no active transaction".into());
        return Vec::new();
    }
    model.transaction_prompt.open = false;
    model.transaction_prompt.error = None;
    effects
}

fn open_data_query_prompt(
    model: &mut Model,
    intent: crate::screens::data::DataQueryIntent,
) -> Vec<Effect> {
    model.data.query_prompt = crate::screens::data::DataQueryPrompt {
        open: true,
        intent: Some(intent),
        ..crate::screens::data::DataQueryPrompt::default()
    };
    Vec::new()
}

fn submit_data_query_prompt(model: &mut Model) -> Vec<Effect> {
    let column = model.data.query_prompt.column.trim().to_string();
    if column.is_empty()
        || !model
            .data
            .table
            .columns
            .iter()
            .any(|col| col.name == column)
    {
        model.data.query_prompt.error = Some("unknown column".into());
        return Vec::new();
    }
    match model.data.query_prompt.intent {
        Some(crate::screens::data::DataQueryIntent::Sort) => {
            model.data.sort = vec![dexo_driver_api::Sort {
                column: dexo_driver_api::ColumnId(column),
                descending: model.data.query_prompt.descending,
            }];
        }
        Some(crate::screens::data::DataQueryIntent::Filter) => {
            model.data.filter = Some(dexo_driver_api::Filter::Eq(
                dexo_driver_api::ColumnId(column),
                dexo_driver_api::DbValue::Text(model.data.query_prompt.value.clone()),
            ));
        }
        None => return Vec::new(),
    }
    model.data.query_prompt.open = false;
    model.data.query_prompt.error = None;
    apply_remote_query(model)
}

fn explain_effect(model: &Model, analyze: bool) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    let document = model.active_document();
    let sql = document.text();
    let cursor = sql
        .chars()
        .take(document.cursor())
        .map(char::len_utf8)
        .sum();
    vec![Effect::RunExplain {
        sql,
        cursor,
        analyze,
        session,
        generation: model.session_generation,
    }]
}

fn save_active_document(model: &mut Model) -> Vec<Effect> {
    let doc = model.active_document();
    match &doc.path {
        Some(path) => vec![Effect::SaveDocument(crate::action::DocumentIoRequest {
            document: doc.id.clone(),
            path: path.clone(),
            content: doc.text(),
            revision: doc.sql.revision(),
            expected_fingerprint: None,
        })],
        None => {
            open_file_picker(model, crate::screens::file_picker::FilePickerMode::Save);
            Vec::new()
        }
    }
}

fn close_active_document(model: &mut Model) -> Vec<Effect> {
    let is_dirty = model.active_document().is_dirty();
    let has_path = model.active_document().path.is_some();
    if is_dirty && !has_path {
        model
            .messages
            .push("Save the untitled document before closing it.".into());
        return Vec::new();
    }
    if is_dirty {
        // The buffer holds the only copy of these edits, so the tab survives
        // until `DocumentSaved` confirms the write. A failed save leaves the
        // tab open with the error in the message log.
        let document = model.active_document();
        model.pending_document_close = Some(crate::model::PendingDocumentClose {
            document: document.id.clone(),
            revision: document.sql.revision(),
        });
        model
            .messages
            .push("Saving dirty file before closing it.".into());
        return save_active_document(model);
    }
    remove_document(model, model.active_document);
    Vec::new()
}

fn remove_document(model: &mut Model, index: usize) {
    if index >= model.documents.len() {
        return;
    }
    model.documents.remove(index);
    if model.documents.is_empty() {
        model
            .documents
            .push(crate::model::EditorDocument::scratch());
        model.active_document = 0;
    } else {
        if model.active_document > index {
            model.active_document -= 1;
        }
        model.active_document = model.active_document.min(model.documents.len() - 1);
    }
    model.focus = Focus::Editor;
}

fn cycle_theme(model: &mut Model) -> Vec<Effect> {
    let (name, theme) = match model.settings.theme.as_str() {
        "light" => ("low-color", crate::theme::builtin_low_color()),
        "low-color" | "high-contrast" => ("dark", crate::theme::builtin_dark()),
        _ => ("light", crate::theme::builtin_light()),
    };
    model.settings.theme = name.into();
    model.theme = theme;
    persist_settings(model);
    Vec::new()
}

fn cycle_keymap(model: &mut Model) -> Vec<Effect> {
    model.keymap = match model.keymap.name.as_str() {
        "vim" => crate::keymap::Keymap::emacs_profile(),
        "emacs" => crate::keymap::Keymap::default_profile(),
        _ => crate::keymap::Keymap::vim_profile(),
    };
    model.settings.keymap = model.keymap.name.clone();
    persist_settings(model);
    Vec::new()
}

fn persist_settings(model: &Model) {
    let Ok(paths) = dexo_storage::AppPaths::discover() else {
        return;
    };
    let mut manager = crate::runtime::settings_manager::SettingsManager::load(&paths.data_dir);
    let next = dexo_app::settings::SettingsFile {
        theme: if model.settings.theme == "high-contrast" || model.settings.theme == "low-color" {
            dexo_app::settings::ThemeId::HighContrast
        } else {
            dexo_app::settings::ThemeId::Dark
        },
        mouse: model.mouse,
        animation: model.animation,
        unicode: if model.capabilities.unicode {
            dexo_app::settings::UnicodeMode::Unicode
        } else {
            dexo_app::settings::UnicodeMode::Ascii
        },
        keymap: dexo_app::settings::KeymapConfig {
            run_statement: "Ctrl+Enter".into(),
        },
        ..manager.active.clone()
    };
    let _ = manager.save(&paths.data_dir, next);
}

fn apply_saved_settings(model: &mut Model) {
    let Ok(paths) = dexo_storage::AppPaths::discover() else {
        return;
    };
    let manager = crate::runtime::settings_manager::SettingsManager::load(&paths.data_dir);
    model.mouse = manager.active.mouse;
    model.settings.mouse = manager.active.mouse;
    model.animation = manager.active.animation;
    match manager.active.theme {
        dexo_app::settings::ThemeId::HighContrast => {
            model.settings.theme = "high-contrast".into();
            model.theme = crate::theme::builtin_low_color();
        }
        dexo_app::settings::ThemeId::Dark => {
            model.settings.theme = "dark".into();
            model.theme = crate::theme::builtin_dark();
        }
    }
    model.settings.keymap = manager.active.keymap.run_statement.clone();
}

fn run_transfer(model: &mut Model) -> Vec<Effect> {
    let path = std::path::PathBuf::from(model.transfer.path.trim());
    if path.as_os_str().is_empty() {
        open_file_picker(model, crate::screens::file_picker::FilePickerMode::Transfer);
        return Vec::new();
    }
    if model.transfer.mode == crate::screens::transfer::TransferMode::Restore
        && !model.transfer.confirm_restore
    {
        model.transfer.confirm_restore = true;
        model.transfer.error = None;
        return Vec::new();
    }
    match build_transfer_request(model, path) {
        Ok(request) => {
            model.transfer.running = true;
            model.transfer.error = None;
            model.transfer.operation = Some(request.operation());
            vec![Effect::RunTransfer(request)]
        }
        Err(message) => {
            model.transfer.error = Some(message);
            Vec::new()
        }
    }
}

fn build_transfer_request(
    model: &Model,
    path: std::path::PathBuf,
) -> Result<crate::action::TransferRequest, String> {
    use crate::action::TransferRequest;
    use crate::screens::transfer::TransferMode;
    let operation = crate::runtime::OperationId::new();
    let format = match model.transfer.format.as_str() {
        "json" => dexo_app::transfer::TransferFormat::Json,
        "tsv" => dexo_app::transfer::TransferFormat::Tsv,
        "jsonl" => dexo_app::transfer::TransferFormat::Jsonl,
        "sql" => dexo_app::transfer::TransferFormat::Sql,
        _ => dexo_app::transfer::TransferFormat::Csv,
    };
    match model.transfer.mode {
        TransferMode::Export => {
            if model.results.rows().is_empty() {
                return Err("no results available".into());
            }
            Ok(TransferRequest::Export {
                operation,
                path,
                format,
                columns: model
                    .results
                    .columns()
                    .iter()
                    .map(|column| column.name.clone())
                    .collect(),
                rows: model.results.rows_snapshot(),
            })
        }
        TransferMode::Import => {
            let session = model.active_session.ok_or("connect a session first")?;
            if model.data.target.object().is_empty() {
                return Err("open a table first".into());
            }
            Ok(TransferRequest::Import {
                operation,
                path,
                format,
                target: model.data.target.clone(),
                strategy: model.transfer.strategy,
                session,
            })
        }
        TransferMode::Backup => {
            let session = model.active_session.ok_or("connect a session first")?;
            Ok(TransferRequest::Backup {
                operation,
                path,
                session,
            })
        }
        TransferMode::Restore => {
            let session = model.active_session.ok_or("connect a session first")?;
            if !model.transfer.confirm_restore {
                return Err("confirm restore first".into());
            }
            Ok(TransferRequest::Restore {
                operation,
                path,
                session,
            })
        }
    }
}

fn open_schema_diff(model: &mut Model) -> Vec<Effect> {
    model.schema_diff.open = true;
    model.schema_diff.source_prompt = true;
    model.schema_diff.entries.clear();
    model.schema_diff.ordered.clear();
    model.schema_diff.left = None;
    model.schema_diff.right = None;
    model.schema_diff.loading = false;
    model.schema_diff.error = None;
    model.schema_diff.confirmed = false;
    model.schema_diff.applied = false;
    Vec::new()
}

fn request_schema_diff(model: &mut Model) -> Vec<Effect> {
    let (Some(left), Some(right), Some(session)) = (
        model.schema_diff.left.clone(),
        model.schema_diff.right.clone(),
        model.active_session,
    ) else {
        model.schema_diff.error = Some("select both schema sources".into());
        return Vec::new();
    };
    model.schema_diff.loading = true;
    model.schema_diff.error = None;
    vec![Effect::LoadSchemaDiff {
        session,
        left,
        right,
        generation: model.session_generation,
    }]
}

fn open_security(model: &mut Model) -> Vec<Effect> {
    model.security.open = true;
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    vec![Effect::LoadSecurity {
        session,
        generation: model.session_generation,
    }]
}

fn open_security_change_preview(model: &mut Model) -> Vec<Effect> {
    let Some(principal) = model
        .security
        .principals
        .get(model.security.selected)
        .cloned()
    else {
        return Vec::new();
    };
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    let change = crate::screens::security::SecurityScreen::grant_select(
        model.data.target.clone(),
        &principal,
    );
    vec![Effect::PreviewDdl {
        change,
        session,
        generation: model.session_generation,
    }]
}

fn open_transfer(model: &mut Model, mode: crate::screens::transfer::TransferMode) -> Vec<Effect> {
    model.transfer.open = true;
    model.transfer.mode = mode;
    model.transfer.running = false;
    model.transfer.error = None;
    model.transfer.message = None;
    model.transfer.confirm_restore = false;
    model.transfer.operation = None;
    Vec::new()
}

fn apply_transfer_progress(
    model: &mut Model,
    operation: crate::runtime::OperationId,
    rows: u64,
    bytes: u64,
) -> Vec<Effect> {
    if model.transfer.operation != Some(operation) {
        return Vec::new();
    }
    model.transfer.progress = dexo_app::transfer::ExportProgress { rows, bytes };
    Vec::new()
}

fn apply_transfer_finished(
    model: &mut Model,
    operation: crate::runtime::OperationId,
    message: String,
) -> Vec<Effect> {
    if model.transfer.operation != Some(operation) {
        return Vec::new();
    }
    model.transfer.running = false;
    model.transfer.message = Some(message);
    Vec::new()
}

fn apply_transfer_failed(
    model: &mut Model,
    operation: crate::runtime::OperationId,
    message: String,
) -> Vec<Effect> {
    if model.transfer.operation != Some(operation) {
        return Vec::new();
    }
    model.transfer.running = false;
    model.transfer.error = Some(message);
    Vec::new()
}

fn handle_history_overlay(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if model.editor.history_confirm_clear {
        return match key.code {
            KeyCode::Esc => {
                model.editor.history_confirm_clear = false;
                Vec::new()
            }
            KeyCode::Enter => confirm_clear_history(model),
            _ => Vec::new(),
        };
    }
    if key.code == KeyCode::Enter {
        return update(model, Action::HistoryPick);
    }
    crate::screens::editor::handle_history_key(model, key);
    Vec::new()
}

fn handle_admin_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.admin.open = false;
            Vec::new()
        }
        KeyCode::Char('p') => update(model, Action::AdminPause),
        KeyCode::Char('r') => update(model, Action::AdminResume),
        KeyCode::Enter => update(model, Action::ConfirmAdmin),
        _ => Vec::new(),
    }
}

fn file_picker_rows(model: &Model) -> usize {
    model
        .height
        .saturating_sub(2)
        .min(22)
        .saturating_sub(5)
        .max(4) as usize
}

fn handle_file_picker_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::file_picker::FilePickerFocus;
    let rows = file_picker_rows(model);
    match key.code {
        KeyCode::Esc => {
            model.file_picker.open = false;
            Vec::new()
        }
        KeyCode::Tab => {
            model.file_picker.focus_next();
            Vec::new()
        }
        KeyCode::BackTab => {
            model.file_picker.focus_prev();
            Vec::new()
        }
        KeyCode::Up if model.file_picker.focus == FilePickerFocus::List => {
            model.file_picker.move_selection(-1, rows);
            Vec::new()
        }
        KeyCode::Down if model.file_picker.focus == FilePickerFocus::List => {
            model.file_picker.move_selection(1, rows);
            Vec::new()
        }
        KeyCode::Left if model.file_picker.focus == FilePickerFocus::List => {
            model.file_picker.parent();
            Vec::new()
        }
        KeyCode::Right if model.file_picker.focus == FilePickerFocus::List => {
            let _ = model.file_picker.activate_selected();
            Vec::new()
        }
        KeyCode::Backspace if model.file_picker.focus == FilePickerFocus::Name => {
            model.file_picker.name.pop();
            Vec::new()
        }
        KeyCode::Backspace if model.file_picker.focus == FilePickerFocus::List => {
            model.file_picker.parent();
            Vec::new()
        }
        KeyCode::Char('h') if model.file_picker.focus == FilePickerFocus::List => {
            model.file_picker.toggle_hidden();
            Vec::new()
        }
        KeyCode::Char(ch)
            if model.file_picker.focus == FilePickerFocus::Name
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            model.file_picker.name.push(ch);
            Vec::new()
        }
        KeyCode::Char(ch)
            if model.file_picker.focus == FilePickerFocus::List
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            model.file_picker.jump_to(ch, rows);
            Vec::new()
        }
        KeyCode::Enter if model.file_picker.focus == FilePickerFocus::Cancel => {
            model.file_picker.open = false;
            Vec::new()
        }
        KeyCode::Enter if model.file_picker.focus == FilePickerFocus::List => {
            if model.file_picker.activate_selected().is_some() {
                file_picker_submit(model)
            } else {
                Vec::new()
            }
        }
        KeyCode::Enter => file_picker_submit(model),
        _ => Vec::new(),
    }
}

fn file_picker_submit(model: &mut Model) -> Vec<Effect> {
    let Some(path) = model.file_picker.chosen_path() else {
        model.file_picker.error = Some("choose a file or type a name".into());
        return Vec::new();
    };
    model.file_picker.open = false;
    match model.file_picker_mode {
        crate::screens::file_picker::FilePickerMode::Open => {
            vec![Effect::LoadDocument(crate::action::DocumentIoRequest {
                document: model.active_document().id.clone(),
                path,
                content: String::new(),
                revision: 0,
                expected_fingerprint: None,
            })]
        }
        crate::screens::file_picker::FilePickerMode::Save => {
            let doc = model.active_document_mut();
            doc.path = Some(path.clone());
            vec![Effect::SaveDocument(crate::action::DocumentIoRequest {
                document: doc.id.clone(),
                path,
                content: doc.text(),
                revision: doc.sql.revision(),
                expected_fingerprint: None,
            })]
        }
        crate::screens::file_picker::FilePickerMode::Transfer => {
            model.transfer.path = path.display().to_string();
            run_transfer(model)
        }
        crate::screens::file_picker::FilePickerMode::Diagnostics => {
            model.diagnostics.path = Some(path.clone());
            model.diagnostics.writing = true;
            model.diagnostics.error = None;
            vec![Effect::WriteDiagnostics {
                path,
                bundle: diagnostics_bundle(model),
            }]
        }
    }
}

fn switch_project(model: &mut Model, name: String) -> Vec<Effect> {
    let target = if name.is_empty() {
        model.projects.selected().cloned()
    } else {
        model.projects.by_name(&name)
    };
    match target {
        Some(project) => start_switch(model, project),
        None if name.is_empty() => vec![Effect::ListProjects],
        None => vec![Effect::SwitchProject { name }],
    }
}

fn start_switch(model: &mut Model, target: dexo_app::Project) -> Vec<Effect> {
    match crate::runtime::project_manager::begin_switch(model, target) {
        Err(message) => {
            model.messages.push(message);
            Vec::new()
        }
        Ok(switch) => {
            model.projects.pending = Some(switch.clone());
            crate::runtime::project_manager::advance(model, &switch)
        }
    }
}

fn complete_switch_stage(model: &mut Model) -> Vec<Effect> {
    let Some(mut switch) = model.projects.pending.clone() else {
        return Vec::new();
    };
    if switch.stage == crate::runtime::project_manager::ProjectSwitchStage::Complete {
        model.projects.pending = None;
        return Vec::new();
    }
    switch.stage = crate::runtime::project_manager::next_stage(switch.stage);
    model.projects.pending = Some(switch.clone());
    crate::runtime::project_manager::advance(model, &switch)
}

fn confirm_project_delete(model: &mut Model) -> Vec<Effect> {
    let Some(delete) = model.projects.delete.take() else {
        return Vec::new();
    };
    if delete.typed != delete.project.name {
        model
            .messages
            .push("type the project name to confirm".into());
        model.projects.delete = Some(delete);
        return Vec::new();
    }
    vec![Effect::DeleteProject {
        id: delete.project.id.0.to_string(),
        delete_connections: delete.delete_connections,
    }]
}

fn apply_loaded_project(
    model: &mut Model,
    project: dexo_app::Project,
    documents: Vec<(String, String)>,
    layout: Option<dexo_storage::WorkbenchLayout>,
) {
    model.project = project.name.clone();
    model.project_id = project.id.0.to_string();
    model.projects.touch_recent(&project.name);
    model.projects.pending = None;
    if documents.is_empty() {
        model.documents = vec![crate::model::EditorDocument::scratch()];
        model.active_document = 0;
    } else {
        model.documents = documents
            .into_iter()
            .map(|(id, content)| {
                let mut document = crate::model::EditorDocument::with_text(&content);
                document.id = id.clone();
                document.title = id;
                document
            })
            .collect();
        model.active_document = 0;
    }
    apply_layout(model, layout);
}

fn handle_projects_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if let Some(delete) = &mut model.projects.delete {
        return match key.code {
            KeyCode::Esc => {
                model.projects.delete = None;
                model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
                Vec::new()
            }
            KeyCode::Enter => update(model, Action::ConfirmProjectDelete),
            KeyCode::Char('c') => {
                delete.delete_connections = !delete.delete_connections;
                Vec::new()
            }
            KeyCode::Backspace => {
                delete.typed.pop();
                Vec::new()
            }
            KeyCode::Char(ch) => {
                delete.typed.push(ch);
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    match model.projects.mode {
        crate::screens::projects::ProjectsMode::Create
        | crate::screens::projects::ProjectsMode::Rename => match key.code {
            KeyCode::Esc => {
                model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
                model.projects.name_input.clear();
                model.projects.error = None;
                model.projects.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            KeyCode::Tab => {
                model.projects.footer = model.projects.footer.next();
                Vec::new()
            }
            KeyCode::BackTab => {
                model.projects.footer = model.projects.footer.prev();
                Vec::new()
            }
            KeyCode::Enter
                if model.projects.footer == crate::widgets::form::FooterFocus::Cancel =>
            {
                model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
                model.projects.name_input.clear();
                model.projects.error = None;
                model.projects.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            KeyCode::Enter => submit_project_name(model),
            KeyCode::Backspace
                if model.projects.footer == crate::widgets::form::FooterFocus::Input =>
            {
                model.projects.name_input.pop();
                Vec::new()
            }
            KeyCode::Char(ch)
                if model.projects.footer == crate::widgets::form::FooterFocus::Input =>
            {
                model.projects.name_input.push(ch);
                Vec::new()
            }
            _ => Vec::new(),
        },
        crate::screens::projects::ProjectsMode::Browse
        | crate::screens::projects::ProjectsMode::DeleteConfirm => match key.code {
            KeyCode::Esc => {
                if model.projects.pending.is_some() {
                    return update(model, Action::CancelProjectSwitch);
                }
                model.projects.open = false;
                model.projects.intent = None;
                model.projects.error = None;
                Vec::new()
            }
            KeyCode::Enter => choose_project_intent(model),
            KeyCode::Up => {
                if model.projects.selected > 0 {
                    model.projects.selected -= 1;
                }
                Vec::new()
            }
            KeyCode::Down => {
                if model.projects.selected + 1 < model.projects.list.len() {
                    model.projects.selected += 1;
                }
                Vec::new()
            }
            KeyCode::Char('n') => {
                model.projects.mode = crate::screens::projects::ProjectsMode::Create;
                model.projects.name_input.clear();
                model.projects.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            KeyCode::Char('r') => {
                model.projects.mode = crate::screens::projects::ProjectsMode::Rename;
                model.projects.name_input = model
                    .projects
                    .selected()
                    .map(|project| project.name.clone())
                    .unwrap_or_default();
                model.projects.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            KeyCode::Char('x') => update(model, Action::DeleteProject),
            KeyCode::Char('y') if model.projects.pending.is_some() => {
                update(model, Action::ConfirmSwitchDirty)
            }
            _ => Vec::new(),
        },
    }
}

fn handle_config_transfer_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.config_transfer.open = false;
            Vec::new()
        }
        KeyCode::Enter => update(model, Action::ApplyConfigImport),
        KeyCode::Char('r') => {
            if let Some(preview) = &model.config_transfer.preview
                && let Some(name) = preview.conflicts.first()
            {
                model.config_transfer.resolutions.insert(
                    name.clone(),
                    dexo_storage::ImportResolution::Rename(format!("{name}-2")),
                );
            }
            Vec::new()
        }
        KeyCode::Char('p') => {
            if let Some(preview) = &model.config_transfer.preview
                && let Some(name) = preview.conflicts.first()
            {
                model
                    .config_transfer
                    .resolutions
                    .insert(name.clone(), dexo_storage::ImportResolution::Replace);
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn open_project_intent(
    model: &mut Model,
    intent: crate::screens::projects::ProjectIntent,
) -> Vec<Effect> {
    model.projects.open = true;
    model.projects.intent = Some(intent);
    model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
    model.projects.error = None;
    vec![Effect::ListProjects]
}

fn open_connection_intent(
    model: &mut Model,
    intent: crate::screens::connections::ConnectionIntent,
) -> Vec<Effect> {
    model.connections.open = true;
    model.connections.intent = Some(intent);
    model.connections.error = None;
    if model.connections.profiles.is_empty() {
        vec![Effect::LoadConnectionProfiles]
    } else {
        Vec::new()
    }
}

fn submit_project_name(model: &mut Model) -> Vec<Effect> {
    let name = model.projects.name_input.trim();
    if name.is_empty() {
        model.projects.error = Some("project name is required".into());
        return Vec::new();
    }
    let name = name.to_string();
    model.projects.error = None;
    match model.projects.mode {
        crate::screens::projects::ProjectsMode::Create => {
            update(model, Action::CreateProject { name })
        }
        crate::screens::projects::ProjectsMode::Rename => {
            update(model, Action::RenameProject { name })
        }
        _ => Vec::new(),
    }
}

fn choose_project_intent(model: &mut Model) -> Vec<Effect> {
    let Some(project) = model.projects.selected().cloned() else {
        model.projects.error = Some("select a project first".into());
        return Vec::new();
    };
    match model.projects.intent {
        Some(crate::screens::projects::ProjectIntent::Switch) => {
            model.projects.intent = None;
            update(model, Action::SwitchProject { name: project.name })
        }
        Some(crate::screens::projects::ProjectIntent::Rename) => {
            model.projects.mode = crate::screens::projects::ProjectsMode::Rename;
            model.projects.name_input = project.name;
            model.projects.error = None;
            Vec::new()
        }
        Some(crate::screens::projects::ProjectIntent::Delete) => {
            model.projects.intent = None;
            update(model, Action::DeleteProject)
        }
        None => update(model, Action::SwitchProject { name: project.name }),
    }
}

fn choose_connection_intent(model: &mut Model) -> Vec<Effect> {
    if model.connections.selected().is_none() {
        model.connections.error = Some("select a connection first".into());
        return Vec::new();
    }
    match model.connections.intent {
        Some(crate::screens::connections::ConnectionIntent::Connect) => {
            update(model, Action::ConnectSelected)
        }
        Some(crate::screens::connections::ConnectionIntent::Duplicate) => {
            update(model, Action::DuplicateConnection)
        }
        Some(crate::screens::connections::ConnectionIntent::Test) => {
            update(model, Action::TestConnection)
        }
        Some(crate::screens::connections::ConnectionIntent::Delete) => {
            update(model, Action::DeleteConnection)
        }
        Some(crate::screens::connections::ConnectionIntent::CloseSession) => {
            update(model, Action::CloseSelectedSession)
        }
        None => connect_selected(model),
    }
}

fn open_snippets(model: &mut Model) -> Vec<Effect> {
    if model.editor.snippets.is_empty() {
        model.editor.snippet_pending = true;
        return vec![Effect::LoadSnippets];
    }
    model.editor.snippet_open = true;
    model.editor.snippet_selected = 0;
    Vec::new()
}

fn open_parameters(model: &mut Model) -> Vec<Effect> {
    crate::screens::editor::refresh_intelligence(model, false);
    if model.editor.parameters.is_empty() {
        model.messages.push("no query parameters".into());
        return Vec::new();
    }
    model.editor.parameter_index = model
        .editor
        .parameters
        .iter()
        .position(|parameter| matches!(parameter.value, DbValue::Null))
        .unwrap_or(0);
    model.editor.parameter_draft.clear();
    model.editor.parameter_prompt = true;
    Vec::new()
}

fn submit_parameter_prompt(model: &mut Model) -> Vec<Effect> {
    if !model.editor.parameter_prompt {
        return Vec::new();
    }
    crate::screens::editor::submit_parameters(model);
    if model.editor.parameter_prompt {
        Vec::new()
    } else {
        start_query(model)
    }
}

fn open_clear_history(model: &mut Model) -> Vec<Effect> {
    model.editor.history_open = true;
    model.editor.history_confirm_clear = true;
    Vec::new()
}

fn confirm_clear_history(model: &mut Model) -> Vec<Effect> {
    let connection_id = model.connection.name.clone();
    model.editor.history_confirm_clear = false;
    vec![Effect::ClearHistory { connection_id }]
}

fn diagnostics_bundle(model: &Model) -> dexo_app::diagnostic_service::DiagnosticBundle {
    dexo_app::diagnostic_service::DiagnosticBundle::assemble(
        env!("CARGO_PKG_VERSION").into(),
        format!("{:?}", model.capabilities),
        format!("theme={} mouse={}", model.settings.theme, model.mouse),
        String::new(),
    )
}

fn open_file_picker(model: &mut Model, mode: crate::screens::file_picker::FilePickerMode) {
    model.file_picker_mode = mode;
    model.file_picker.open_browser();
    if mode == crate::screens::file_picker::FilePickerMode::Save
        && let Some(path) = model.active_document().path.as_ref()
        && let Some(name) = path.file_name()
    {
        model.file_picker.name = name.to_string_lossy().into();
    }
}

fn open_diagnostics_picker(model: &mut Model) -> Vec<Effect> {
    open_file_picker(
        model,
        crate::screens::file_picker::FilePickerMode::Diagnostics,
    );
    Vec::new()
}

fn invoke_palette(model: &mut Model, invocation: crate::palette::PaletteInvocation) -> Vec<Effect> {
    use crate::palette::{FlowIntent, PaletteInvocation};
    match invocation {
        PaletteInvocation::Dispatch(action) => update(model, action),
        PaletteInvocation::OpenFlow(FlowIntent::ProjectCreate) => {
            model.projects.open = true;
            model.projects.mode = crate::screens::projects::ProjectsMode::Create;
            model.projects.intent = None;
            model.projects.error = None;
            model.projects.name_input.clear();
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectSwitch) => {
            open_project_intent(model, crate::screens::projects::ProjectIntent::Switch)
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectRename) => {
            open_project_intent(model, crate::screens::projects::ProjectIntent::Rename)
        }
        PaletteInvocation::OpenFlow(FlowIntent::ProjectDelete) => {
            open_project_intent(model, crate::screens::projects::ProjectIntent::Delete)
        }
        PaletteInvocation::OpenFlow(FlowIntent::SavepointCreate) => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Create,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::SavepointRollback) => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Rollback,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::SavepointRelease) => open_savepoint_prompt(
            model,
            crate::screens::transaction_prompt::SavepointIntent::Release,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::DataSort) => {
            open_data_query_prompt(model, crate::screens::data::DataQueryIntent::Sort)
        }
        PaletteInvocation::OpenFlow(FlowIntent::DataFilter) => {
            open_data_query_prompt(model, crate::screens::data::DataQueryIntent::Filter)
        }
        PaletteInvocation::OpenFlow(FlowIntent::DataReview) => update(model, Action::OpenReview),
        PaletteInvocation::OpenFlow(FlowIntent::SchemaPreview) => {
            update(model, Action::OpenDdlPreview)
        }
        PaletteInvocation::OpenFlow(FlowIntent::SchemaRaw) => update(model, Action::ApplyRawDdl),
        PaletteInvocation::OpenFlow(FlowIntent::SchemaDiff) => {
            update(model, Action::OpenSchemaDiff)
        }
        PaletteInvocation::OpenFlow(FlowIntent::Security) => update(model, Action::OpenSecurity),
        PaletteInvocation::OpenFlow(FlowIntent::TransferExport) => {
            open_transfer(model, crate::screens::transfer::TransferMode::Export)
        }
        PaletteInvocation::OpenFlow(FlowIntent::TransferImport) => {
            open_transfer(model, crate::screens::transfer::TransferMode::Import)
        }
        PaletteInvocation::OpenFlow(FlowIntent::Backup) => {
            open_transfer(model, crate::screens::transfer::TransferMode::Backup)
        }
        PaletteInvocation::OpenFlow(FlowIntent::Restore) => {
            open_transfer(model, crate::screens::transfer::TransferMode::Restore)
        }
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionConnect) => open_connection_intent(
            model,
            crate::screens::connections::ConnectionIntent::Connect,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionDuplicate) => open_connection_intent(
            model,
            crate::screens::connections::ConnectionIntent::Duplicate,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionTest) => {
            open_connection_intent(model, crate::screens::connections::ConnectionIntent::Test)
        }
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionDelete) => {
            open_connection_intent(model, crate::screens::connections::ConnectionIntent::Delete)
        }
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionCloseSession) => open_connection_intent(
            model,
            crate::screens::connections::ConnectionIntent::CloseSession,
        ),
        PaletteInvocation::OpenFlow(FlowIntent::SettingsReset) => {
            model.settings.open = true;
            model.settings.confirm_reset = true;
            Vec::new()
        }
        PaletteInvocation::OpenFlow(FlowIntent::RecoveryRestore) => {
            model.recovery.open = true;
            model.recovery.confirm_discard = false;
            Vec::new()
        }
        PaletteInvocation::OpenFlow(FlowIntent::RecoveryDiscard) => {
            model.recovery.open = true;
            model.recovery.confirm_discard = true;
            Vec::new()
        }
        PaletteInvocation::OpenFlow(FlowIntent::McpRevokeAll) => {
            update(model, Action::RevokeAllMcpGrants)
        }
        PaletteInvocation::OpenFlow(FlowIntent::InsertSnippet) => open_snippets(model),
        PaletteInvocation::OpenFlow(FlowIntent::SubmitParameters) => open_parameters(model),
        PaletteInvocation::OpenFlow(FlowIntent::ClearHistory) => open_clear_history(model),
        PaletteInvocation::OpenFlow(FlowIntent::DiagnosticsExport) => {
            update(model, Action::OpenDiagnostics)
        }
    }
}

fn palette_select(model: &mut Model) -> Vec<Effect> {
    let entries = crate::palette::palette_entries(model);
    let visible = crate::palette::filter_entries(&entries, &model.palette.query);
    let Some(entry) = visible.get(model.palette.selected) else {
        return Vec::new();
    };
    if let Some(reason) = &entry.disabled_reason {
        model.messages.push(reason.clone());
        return Vec::new();
    }
    let invocation = entry.invocation.clone();
    close_palette(model);
    invoke_palette(model, invocation)
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
                index: 0,
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

    #[test]
    fn rollback_savepoint_emits_effect() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use dexo_driver_api::TransactionState;
        let mut model = Model {
            transaction: TransactionState::Active,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        assert!(update(&mut model, Action::RollbackSavepoint).is_empty());
        assert!(model.transaction_prompt.open);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        let effects = update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(
            &effects[..],
            [Effect::RollbackToSavepoint { name, .. }] if name == "x"
        ));
        update(&mut model, Action::ReleaseSavepoint);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        let effects = update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(
            &effects[..],
            [Effect::ReleaseSavepoint { name, .. }] if name == "x"
        ));
    }

    #[test]
    fn new_document_binds_active_connection_uuid() {
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

        let connection_uuid = uuid::Uuid::from_u128(42);
        let profile = ConnectionProfile::new(
            ConnectionId(connection_uuid),
            None,
            "prod",
            "postgres",
            "local",
            serde_json::json!({"host": "localhost"}),
            SecretRef::new("ref-1".into()),
        );
        let mut model = Model::default();
        model.connections.load_profiles(vec![profile]);
        model.connection.name = "prod".into();

        update(&mut model, Action::NewDocument);

        let doc = model.documents.last().unwrap();
        assert_eq!(
            doc.connection_id.as_deref(),
            Some(connection_uuid.to_string().as_str())
        );
        assert_ne!(doc.id, "scratch");
    }

    #[test]
    fn save_document_without_path_opens_picker() {
        let mut model = Model::default();
        update(&mut model, Action::SaveActiveDocument);
        assert!(model.file_picker.open);
    }

    fn catalog_object(
        id: &str,
        kind: dexo_driver_api::ObjectKind,
        name: &str,
    ) -> dexo_driver_api::CatalogObject {
        dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new(id),
            kind,
            dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    }

    #[test]
    fn explorer_enter_opens_table_data_and_inspector() {
        use dexo_driver_api::{CatalogList, ObjectId, ObjectKind};

        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        model.explorer.replace_roots(CatalogList {
            objects: vec![catalog_object("table:orders", ObjectKind::Table, "orders")],
            restrictions: vec![],
        });
        model.explorer.select(ObjectId::new("table:orders"));
        let effects = update(&mut model, Action::ExplorerExpand);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTableData { .. }))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadObjectInspector { .. }))
        );
        assert!(model.inspector.open);
        assert_eq!(model.focus, Focus::Results);
    }

    #[test]
    fn explorer_enter_on_schema_still_expands() {
        use dexo_driver_api::{CatalogList, ObjectId, ObjectKind};

        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        model.explorer.replace_roots(CatalogList {
            objects: vec![catalog_object(
                "schema:public",
                ObjectKind::Schema,
                "public",
            )],
            restrictions: vec![],
        });
        model.explorer.select(ObjectId::new("schema:public"));
        let effects = update(&mut model, Action::ExplorerExpand);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadCatalogChildren { .. }))
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTableData { .. }))
        );
        assert!(
            model
                .explorer
                .selected_node()
                .is_some_and(|node| node.expanded)
        );
        let again = update(&mut model, Action::ExplorerExpand);
        assert!(again.is_empty());
        assert!(
            model
                .explorer
                .selected_node()
                .is_some_and(|node| !node.expanded)
        );
    }

    #[test]
    fn results_right_increments_column_offset_each_key() {
        use crate::model::GridSelection;
        use dexo_driver_api::ColumnMeta;

        let mut model = Model::default();
        model.results.set_columns(
            (0..8)
                .map(|i| ColumnMeta {
                    name: format!("wide_column_name_{i:02}"),
                    type_name: "text".into(),
                    nullable: true,
                })
                .collect(),
        );
        model.results.append_rows(vec![
            (0..8).map(|_| DbValue::Text("x".repeat(40))).collect(),
            (0..8).map(|_| DbValue::Text("y".repeat(40))).collect(),
        ]);
        model.results.set_viewport_size(20, 4);
        model.results.select_row(0);
        update(&mut model, Action::ResultsRight);
        assert_eq!(model.results.viewport().column_offset, 1);
        update(&mut model, Action::ResultsRight);
        assert_eq!(model.results.viewport().column_offset, 2);
        model.results.move_cursor_row(1, true);
        let before = model.results.kind.clone();
        update(&mut model, Action::ResultsRight);
        assert_eq!(
            std::mem::discriminant(&model.results.kind),
            std::mem::discriminant(&before)
        );
        assert!(matches!(before, GridSelection::Range { .. }));
    }
}
