use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use dexo_app::data::{inspect_value, related_filter};
use dexo_driver_api::{DbValue, QueryRequest, TransactionState};

use crate::action::{Action, Effect, FocusTarget};
use crate::layout::LayoutPlan;
use crate::model::{DragKind, DragState, Focus, Model};
use crate::mouse::{HitButton, HitTarget, OverlayKind, PaneEdge, note_click, top_overlay};
use ratatui::layout::{Position, Rect};

/// The grid's pane depends on which document is active: a table document draws it in
/// the editor's slot and leaves the bottom pane holding only the console. Every path
/// that swaps the active document -- the tree, the tab strip, Alt+Left/Right, closing a
/// document, restoring a layout -- has to re-derive the row viewport from the new pane,
/// or the cursor keeps scrolling against the pane it was sized for. There is no one
/// place that assigns `active_document`, so the check lives where they all return.
pub fn update(model: &mut Model, action: Action) -> Vec<Effect> {
    let was_table = model.active_document().kind.is_table();
    // Same reason as the viewport below: the document can change under any action, so
    // the output pane follows it here rather than at each of the assignment sites.
    // Before, so the action writes into the active document's pane; after, because the
    // action may have switched documents and the next frame draws before the next
    // action arrives.
    // Before the action as well as after it: text can land in the stand-in outside an
    // action -- restored recovery, a test harness -- and an action like a project switch
    // flushes before anything after it could run.
    promote_placeholder(model);
    let mut swapped = model.swap_results_to_active_document();
    let effects = dispatch(model, action);
    promote_placeholder(model);
    model.drop_redundant_placeholder();
    swapped |= model.swap_results_to_active_document();
    model.follow_active_document_tab();
    if model.editor.completion_open
        && (model.focus != Focus::Editor
            || model.palette.open
            || crate::screens::editor::completion_went_stale(model))
    {
        crate::screens::editor::close_completion(model);
    }
    // Switching tabs left the previous document's colours painted over the new one
    // until the next edit, and a file loaded from disk came up uncoloured.
    if !crate::screens::editor::highlights_are_current(model) {
        crate::screens::editor::refresh_intelligence(model, false);
    }
    crate::screens::editor::follow_cursor(model);
    if swapped || model.active_document().kind.is_table() != was_table {
        model.sync_grid_viewport();
    }
    effects
}

fn dispatch(model: &mut Model, action: Action) -> Vec<Effect> {
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
            let mut answered_connect = false;
            if let Some(pending) = model.connections.pending_connect {
                if token != pending {
                    return Vec::new();
                }
                model.connections.pending_connect = None;
                answered_connect = true;
            }
            if answered_connect && ready {
                // Replaces the "Connecting…" toast; leaving that one up would read as a
                // dial that never finished.
                model.messages.info(format!("Connected to {name}"));
            }
            // An execution that was waiting on this connection. Dropped rather than
            // deferred again if the token moved on or the user changed tabs, because
            // firing a query at a document the user has left is worse than not firing.
            let replay = model
                .pending_execute
                .take()
                .filter(|pending| ready && pending.token == token)
                .filter(|pending| model.active_document().id == pending.document)
                .map(|pending| pending.action);
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
                        connection: name.clone(),
                        transaction: TransactionState::Idle,
                        generation,
                        environment: model.connection.environment.clone(),
                        read_only,
                        driver: model.connection.driver.clone(),
                    });
                model.connections.selected_session = Some(id);
            }
            model
                .explorer
                .sync_connection_roots(&model.connections.profiles, model.connection.name.as_str());
            if ready {
                model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
                let connection = crate::screens::explorer::connection_id(&model.connection.name);
                model.explorer.select(connection.clone());
                let mut effects = Vec::new();
                if let Some(session) = session {
                    let operation = crate::runtime::OperationId::new();
                    model.explorer.expand_with(&connection, operation);
                    effects.push(Effect::LoadCatalogChildren {
                        parent: Some(connection),
                        operation,
                        session,
                        generation,
                        replace_roots: false,
                        include_system: model.explorer.include_system,
                    });
                }
                if let Some(action) = replay {
                    effects.extend(update(model, action));
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
        Action::OpenConnectionForm => {
            model.connection_form = crate::screens::connection::ConnectionForm::open();
            Vec::new()
        }
        Action::ConnectionFormError { message } => {
            // Connecting from the sidebar leaves the form closed, and an error written
            // to a closed widget is an error the user never sees.
            model.connections.pending_connect = None;
            if model.connection_form.open {
                model.connection_form.set_error(message);
            } else {
                model.messages.error(message);
            }
            Vec::new()
        }
        Action::SessionOpened { token } => vec![Effect::AdoptSession { token }],
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
                model.messages.info(message);
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
        Action::CheckpointTick => checkpoint_session(model),
        Action::OnboardingTick => {
            if model.onboarding.open && model.onboarding.logo_frames.len() > 1 {
                model.onboarding.logo_frame =
                    (model.onboarding.logo_frame + 1) % model.onboarding.logo_frames.len();
            }
            Vec::new()
        }
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
            model.messages.error(message);
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
            if !model.connections.open
                && model.focus == Focus::Explorer
                && let Some(index) = selected_connection_profile_index(model)
            {
                model.connections.selected_profile = index;
            }
            match model.connections.selected().cloned() {
                Some(profile) => {
                    model.connection_form =
                        crate::screens::connection::ConnectionForm::open_edit(&profile);
                }
                None => model
                    .messages
                    .warn("No saved connection to edit — press n to add one.".into()),
            }
            Vec::new()
        }
        Action::EditConnectionGroup => {
            let effects = update(model, Action::EditSelectedConnection);
            // The form owns the only text input for a group, so "move to group" is that
            // form opened with the cursor already there.
            if let Some(index) = model
                .connection_form
                .fields
                .iter()
                .position(|field| field.label == "group")
            {
                model.connection_form.focus = index;
            }
            effects
        }
        Action::OpenNodeMenu => {
            open_node_menu(model);
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
            let target = model.connections.selected().cloned();
            model.connections.ask_delete(target);
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
        Action::CloseSelectedSession => close_selected_session(model),
        Action::ProfilesLoaded(profiles) => {
            model.connections.load_profiles(profiles);
            sync_explorer_connections(model);
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
            sync_explorer_connections(model);
            model.messages.info(format!("saved {}", profile.name));
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
                model.explorer.offline = false;
                model.explorer.stale = false;
            }
            sync_explorer_connections(model);
            model.messages.info(format!("deleted {name}"));
            effects
        }
        Action::ConnectionTested { name, ok, message } => {
            if ok {
                model.messages.info(format!("{name} ok"));
            } else {
                model.messages.error(format!("{name}: {message}"));
            }
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
        Action::ExecuteStatement | Action::ExecuteSelection | Action::ExecuteDocument => {
            execute_on_document_connection(model, action)
        }
        Action::CancelQuery => cancel_query(model),
        Action::BeginTransaction => {
            if model.connection.read_only {
                model.messages.warn("connection is read-only".into());
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
        Action::Focus(target) => focus_pane(model, target),
        Action::ActivateDocumentTab => activate_document_tab(model),
        Action::Paste(text) => {
            if crate::screens::editor::paste(model, &text) {
                crate::screens::editor::refresh_intelligence(model, false);
                return crate::screens::editor::take_completion_effects(model);
            }
            // Anywhere else -- a form field, a prompt -- the text is short and the
            // widget only knows keys, so it is fed as the keys it stands for.
            let mut effects = Vec::new();
            for ch in text.chars().filter(|ch| !ch.is_control()) {
                effects.extend(update(
                    model,
                    Action::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                ));
            }
            effects
        }
        Action::PasteFromClipboard => vec![Effect::ReadClipboard],
        Action::EditorCopy => crate::screens::editor::copy(model)
            .map(|text| vec![Effect::CopyToClipboard { text }])
            .unwrap_or_default(),
        Action::EditorCut => {
            let effects = crate::screens::editor::cut(model)
                .map(|text| vec![Effect::CopyToClipboard { text }])
                .unwrap_or_default();
            crate::screens::editor::refresh_intelligence(model, false);
            effects
        }
        Action::MoveDocumentTabCursor(delta) => {
            model.move_tab_cursor(delta);
            Vec::new()
        }
        Action::ExplorerExpand => activate_connection_or_catalog(model),
        Action::RefreshCatalogNode => refresh_catalog(model, false),
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
            model.absorb_catalog(&list.objects);
            let capture = replace_roots
                || parent.as_ref()
                    == Some(&crate::screens::explorer::connection_id(
                        &model.connection.name,
                    ));
            if capture {
                if let Some(parent) = parent {
                    model.explorer.apply_children(&parent, list);
                } else if !model.connection.name.is_empty() {
                    model.explorer.replace_connection_catalog(
                        &model.connection.name,
                        list,
                        model.explorer.offline,
                    );
                } else {
                    model.explorer.replace_roots(list);
                }
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
                    model.messages.error(message);
                }
            }
            Vec::new()
        }
        Action::OpenObjectInspector => open_inspector_facet(
            model,
            crate::screens::object_inspector::InspectorFacet::Properties,
        ),
        Action::OpenObjectDdl => {
            open_inspector_facet(model, crate::screens::object_inspector::InspectorFacet::Ddl)
        }
        Action::OpenObjectData => open_selected_table(model),
        Action::OpenDependencies => open_inspector_facet(
            model,
            crate::screens::object_inspector::InspectorFacet::Properties,
        ),
        Action::ExplorerUp => {
            move_sidebar_selection(model, -1);
            Vec::new()
        }
        Action::ExplorerDown => {
            move_sidebar_selection(model, 1);
            Vec::new()
        }
        Action::SelectDocument { index } => {
            if index < model.documents.len() {
                model.document_tab_focus = crate::model::DocumentTabFocus::Document(index);
                model.focus = Focus::Editor;
                let effects = activate_document(model, index);
                model.sync_document_tabs_scroll();
                return effects;
            }
            Vec::new()
        }
        Action::NextDocument => {
            if !model.documents.is_empty() {
                let index = (model.active_document + 1) % model.documents.len();
                let effects = activate_document(model, index);
                model.focus_active_document_tab();
                model.focus = Focus::Editor;
                return effects;
            }
            Vec::new()
        }
        Action::PrevDocument => {
            if !model.documents.is_empty() {
                let index = model
                    .active_document
                    .checked_sub(1)
                    .unwrap_or(model.documents.len() - 1);
                let effects = activate_document(model, index);
                model.focus_active_document_tab();
                model.focus = Focus::Editor;
                return effects;
            }
            Vec::new()
        }
        // Gated on the editor focus while the strip was only drawn there. It is always
        // on screen now, and a table document never holds the editor focus at all.
        Action::NextDocumentTabFocus => {
            model.advance_document_tab_focus(1);
            Vec::new()
        }
        Action::PrevDocumentTabFocus => {
            model.advance_document_tab_focus(-1);
            Vec::new()
        }
        Action::ScrollDocumentTabsPrev => {
            model.document_tabs_scroll = model.document_tabs_scroll.saturating_sub(1);
            Vec::new()
        }
        Action::ScrollDocumentTabsNext => {
            if !model.documents.is_empty() {
                model.document_tabs_scroll =
                    (model.document_tabs_scroll + 1).min(model.documents.len().saturating_sub(1));
            }
            Vec::new()
        }
        Action::CloseDocument => close_active_document(model),
        Action::ResolveClose(choice) => resolve_close(model, choice),
        Action::NewDocument => {
            open_new_document_prompt(model);
            Vec::new()
        }
        Action::RenameDocument => {
            open_rename_document_prompt(model);
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
        Action::CycleMode => cycle_mode(model, 1),
        Action::CycleAccent => cycle_accent(model, 1),
        Action::CycleKeymap => cycle_keymap(model, 1),
        Action::ToggleMouse => {
            model.mouse = !model.mouse;
            model.settings.mouse = model.mouse;
            persist_settings(model);
            Vec::new()
        }
        Action::ToggleAnimation => {
            model.animation = !model.animation;
            model.settings.animation = model.animation;
            persist_settings(model);
            Vec::new()
        }
        Action::ToggleUnicode => {
            model.capabilities.unicode = !model.capabilities.unicode;
            model.settings.unicode = model.capabilities.unicode;
            persist_settings(model);
            Vec::new()
        }
        Action::ToggleUpdateCheck => {
            model.settings.updates = !model.settings.updates;
            if !model.settings.updates {
                model.update_notice = None;
            }
            persist_settings(model);
            Vec::new()
        }
        Action::UpdateAvailable { version, command } => {
            model.messages.info(format!(
                "Dexo {version} is available. Update with: {command}"
            ));
            model.update_notice = Some(crate::model::UpdateNotice { version, command });
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
                let row_count = page.rows.len();
                model.results.append_rows(page.rows);
                promote_remote_cells(model, &page.columns);
                log_rows_retrieved(model, row_count);
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
                model.messages.error(message);
            }
            Vec::new()
        }
        Action::TableColumnsLoaded {
            generation,
            columns,
        } => {
            if generation == model.session_generation {
                model.data.table = dexo_app::data::TableMeta {
                    columns: columns
                        .into_iter()
                        .map(|column| dexo_app::data::ColumnDef {
                            name: column.name,
                            primary_key: column.primary_key,
                            unique: column.unique,
                            nullable: true,
                        })
                        .collect(),
                };
                model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
                model.data.row_changes.clear();
            }
            Vec::new()
        }
        Action::TableColumnsFailed {
            generation,
            message,
        } => {
            if generation == model.session_generation {
                model.messages.error(message);
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
                model.data.row_changes.clear();
                return load_table_document(model, model.active_document);
            }
            Vec::new()
        }
        Action::MutationsFailed {
            generation,
            message,
        } => {
            if generation == model.session_generation {
                model.data.fail_apply(message.clone());
                model.messages.error(message);
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
            // Said out loud: a copy that went nowhere used to look exactly like one that
            // worked.
            let lines = text.lines().count().max(1);
            model.messages.info(if lines == 1 {
                "copied to clipboard".into()
            } else {
                format!("copied {lines} lines to clipboard")
            });
            model.explorer.copied = Some(text.clone());
            model.data.clipboard = text;
            Vec::new()
        }
        Action::ClipboardFailed { message } => {
            model.messages.error(message);
            Vec::new()
        }
        Action::OfflineCatalogLoaded {
            generation,
            list,
            created_at,
        } => {
            if generation == model.session_generation {
                model.absorb_catalog(&list.objects);
                if model.connection.name.is_empty() {
                    model.explorer.replace_roots(list);
                } else {
                    let connection_name = model.connection.name.clone();
                    model
                        .explorer
                        .restore_connection_catalog(&connection_name, list);
                }
                if let Some(created_at) = created_at {
                    model
                        .messages
                        .info(format!("offline catalog from {created_at}"));
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
            model.data.fail_apply("apply failed".into());
            Vec::new()
        }
        Action::RevertChanges => {
            model.data.revert();
            Vec::new()
        }
        Action::DiscardAllChanges => {
            discard_all_pending(model);
            Vec::new()
        }
        // Offline, it dials the table's own connection and runs once the session lands.
        Action::RefreshTableData => execute_on_document_connection(model, action),
        Action::ToggleRowDelete => toggle_row_delete(model),
        Action::OpenInsertRow => {
            model.data.insert_form.open_for(&model.data.table);
            Vec::new()
        }
        Action::CancelInsertRow => {
            model.data.insert_form.close();
            Vec::new()
        }
        Action::SubmitInsertRow => submit_insert_row(model),
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
                model.schema_editor.open = true;
            } else {
                model.messages.warn("no SQL to apply".into());
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
            model.messages.error(message);
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
            model.results.view = crate::model::ResultsView::Explain;
            model.results.explain_scroll = 0;
            explain_effect(model, false)
        }
        Action::DismissToast => {
            model.messages.dismiss();
            Vec::new()
        }
        Action::ToastTick => {
            model.messages.tick();
            Vec::new()
        }
        Action::CycleResultsView => {
            // One flat ring over everything the output pane can show, so the user has a
            // single question to answer instead of two nested ones.
            use crate::model::ResultsView;
            use crate::screens::explain::ExplainView;
            model.results.explain_scroll = 0;
            model.results.messages_scroll = 0;
            match (model.results.view, model.explain.view) {
                (ResultsView::Grid, _) => {
                    model.results.view = ResultsView::Explain;
                    model.explain.view = ExplainView::Tree;
                }
                (ResultsView::Explain, ExplainView::Summary) => {
                    model.results.view = ResultsView::Messages;
                }
                (ResultsView::Explain, view) => model.explain.view = view.next(),
                (ResultsView::Messages, _) => model.results.view = ResultsView::Grid,
            }
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
        Action::ToggleMcpProfile => match model.mcp_profiles.toggle_selected() {
            Some(enabled) => vec![Effect::SetMcpProfileEnabled {
                name: model.mcp_profiles.name.clone(),
                enabled,
            }],
            None => Vec::new(),
        },
        Action::RevokeProfileGrants => match model.mcp_profiles.revoke_profile() {
            Some(profile) => vec![Effect::RevokeMcpGrants { profile }],
            None => Vec::new(),
        },
        Action::RevokeAllMcpGrants => {
            model.mcp_audit.open = false;
            model.mcp_profiles.open = true;
            model.mcp_profiles.confirm_revoke =
                Some(crate::screens::mcp_profiles::RevokeScope::All);
            model.mcp_profiles.preview = "confirm revoke all grants".into();
            Vec::new()
        }
        Action::McpGrantsRevoked { count } => {
            model.mcp_profiles.grants.clear();
            model.mcp_profiles.confirm_revoke = None;
            model.mcp_profiles.preview = format!("revoked {count} grants");
            Vec::new()
        }
        Action::McpRevokeFailed { message } => {
            model.mcp_profiles.preview = message;
            Vec::new()
        }
        Action::OpenSettings => {
            model.settings.open = true;
            model.settings.focus = 0;
            model.settings.confirm_reset = false;
            sync_settings_screen(model);
            Vec::new()
        }
        Action::ConfirmResetSettings => {
            if !model.settings.open {
                model.settings.open = true;
            }
            if !model.settings.confirm_reset {
                model.settings.confirm_reset = true;
            } else {
                reset_settings_to_defaults(model);
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
            crate::screens::editor::take_completion_effects(model)
        }
        Action::CompletionObjectsLoaded {
            document,
            revision,
            objects,
        } => {
            crate::screens::editor::merge_completion_objects(model, &document, revision, objects);
            Vec::new()
        }
        Action::CompletionCatalogLoaded {
            generation,
            objects,
            complete,
        } => {
            if generation != model.session_generation {
                return Vec::new();
            }
            if complete {
                model.catalog_objects.clear();
                model.catalog_revision = model.catalog_revision.wrapping_add(1);
                model.absorb_catalog(&objects);
            } else {
                let known: std::collections::HashSet<_> = model
                    .catalog_objects
                    .iter()
                    .map(|object| object.id.clone())
                    .collect();
                let missing: Vec<_> = objects
                    .into_iter()
                    .filter(|object| !known.contains(&object.id))
                    .collect();
                model.absorb_catalog(&missing);
            }
            crate::screens::editor::refresh_waiting_completion(model);
            crate::screens::editor::take_completion_effects(model)
        }
        Action::CompletionColumnsLoaded {
            generation,
            target,
            columns,
        } => {
            if generation != model.session_generation {
                return Vec::new();
            }
            crate::screens::editor::absorb_completion_columns(model, &target, &columns);
            crate::screens::editor::refresh_waiting_completion(model);
            crate::screens::editor::take_completion_effects(model)
        }
        Action::FormatSql => {
            crate::screens::editor::apply_format(model);
            Vec::new()
        }
        Action::EditorUndo => {
            crate::screens::editor::undo(model);
            crate::screens::editor::refresh_intelligence(model, false);
            Vec::new()
        }
        Action::EditorRedo => {
            crate::screens::editor::redo(model);
            crate::screens::editor::refresh_intelligence(model, false);
            Vec::new()
        }
        Action::EditorSelectAll => {
            crate::screens::editor::select_all(model);
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
                model.messages.warn("no snippets available".into());
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
            model.messages.info(message);
            model.schema_editor.preview = None;
            Vec::new()
        }
        Action::ExplainLoaded { plan } => {
            let previous = model.explain.plan.clone();
            model.explain.set_plan(*plan, previous.as_ref());
            // Show the plan where output lives; never move the user's focus for it.
            model.results.view = crate::model::ResultsView::Explain;
            model.results.explain_scroll = 0;
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
            model.messages.info(preview);
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
        Action::DocumentLoaded {
            document,
            path,
            content,
        } => {
            if let Some(doc) = model.documents.iter_mut().find(|item| item.id == document) {
                let title = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| doc.title.clone());
                // Only the text arrives. Rebuilding the tab dropped the connection it
                // belongs to.
                doc.sql = dexo_sql::SqlDocument::new(&content);
                doc.title = title;
                doc.path = Some(path.clone());
                doc.saved_revision = doc.sql.revision();
            }
            touch_recent_sql_file(model, &path)
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
            let saved_path = model
                .documents
                .iter()
                .find(|candidate| candidate.id == document)
                .and_then(|candidate| candidate.path.clone());
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

            let mut effects = Vec::new();
            if let Some(path) = saved_path {
                effects.extend(touch_recent_sql_file(model, &path));
            }

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
            effects
        }
        Action::DocumentConflict { path } => {
            // The write never landed, so any tab waiting on it stays open.
            model.pending_document_close = None;
            model
                .messages
                .error(format!("file changed on disk: {path}"));
            Vec::new()
        }
        Action::ResultsUp => {
            match model.results.view {
                crate::model::ResultsView::Explain => {
                    model.results.explain_scroll = model.results.explain_scroll.saturating_sub(1);
                }
                crate::model::ResultsView::Messages => {
                    model.results.messages_scroll = model.results.messages_scroll.saturating_sub(1);
                }
                crate::model::ResultsView::Grid => model.results.move_cursor_row(-1, false),
            }
            Vec::new()
        }
        Action::ResultsDown => {
            match model.results.view {
                crate::model::ResultsView::Explain => {
                    model.results.explain_scroll = model.results.explain_scroll.saturating_add(1);
                }
                crate::model::ResultsView::Messages => {
                    // Bounded by the log itself; it is the one list here that only grows.
                    model.results.messages_scroll = model
                        .results
                        .messages_scroll
                        .saturating_add(1)
                        .min(model.messages.len().saturating_sub(1) as u16);
                }
                crate::model::ResultsView::Grid => model.results.move_cursor_row(1, false),
            }
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
        Action::HideExplorer => {
            model.panes.explorer_visible = !model.panes.explorer_visible;
            if !model.panes.explorer_visible && model.focus == Focus::Explorer {
                model.focus = Focus::Editor;
            }
            model.panes = model.panes.clamp(model.width, model.height);
            model.layout_dirty = true;
            model.sync_grid_viewport();
            Vec::new()
        }
        Action::HideResults => {
            model.panes.results_visible = !model.panes.results_visible;
            if !model.panes.results_visible && model.focus == Focus::Results {
                model.focus = Focus::Editor;
            }
            model.panes = model.panes.clamp(model.width, model.height);
            model.layout_dirty = true;
            model.sync_grid_viewport();
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
        Action::Quit => {
            let mut effects = checkpoint_dirty(model);
            effects.push(flush_documents_effect(model));
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
            recent_sql_files,
        } => {
            apply_loaded_project(model, project, documents, layout, recent_sql_files);
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
            model.messages.error(message);
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

/// Enter on the strip. On `+` it starts a new document; on a tab it hands focus to the
/// document, which is where the user was heading. This was a special case in
/// `handle_key` guarded on `focus == Editor`, so from the results pane the key did
/// nothing at all.
fn activate_document_tab(model: &mut Model) -> Vec<Effect> {
    match model.document_tab_focus {
        crate::model::DocumentTabFocus::New => update(model, Action::NewDocument),
        crate::model::DocumentTabFocus::Document(index) => {
            let mut effects = activate_document(model, index);
            effects.extend(focus_pane(model, FocusTarget::Editor));
            effects
        }
    }
}

fn focus_pane(model: &mut Model, target: FocusTarget) -> Vec<Effect> {
    crate::screens::editor::end_typing(model);
    let leaving_editor = model.focus == Focus::Editor && !matches!(target, FocusTarget::Editor);
    // A table document shifts everything down a pane: the grid takes the editor's slot
    // and the console takes the grid's. These keys name a position, not a widget, and
    // only pane 3 needs saying here -- `effective_focus` already reads pane 2 correctly.
    let table = model.active_document().kind.is_table();
    model.focus = match target {
        FocusTarget::Explorer => {
            model.panes.explorer_visible = true;
            Focus::Explorer
        }
        FocusTarget::Editor => Focus::Editor,
        FocusTarget::DocumentTabs => {
            model.sync_document_tab_focus();
            Focus::DocumentTabs
        }
        FocusTarget::Results => {
            model.panes.results_visible = true;
            if table {
                Focus::Console
            } else {
                Focus::Results
            }
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

fn handle_mouse(model: &mut Model, mouse: MouseEvent) -> Vec<Effect> {
    if model.onboarding.open && !matches!(mouse.kind, MouseEventKind::Down(_)) {
        return Vec::new();
    }
    if !model.mouse {
        return Vec::new();
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => handle_mouse_down(model, mouse),
        MouseEventKind::Down(MouseButton::Right) => handle_mouse_right_down(model, mouse),
        MouseEventKind::Drag(MouseButton::Left) if model.drag.is_some() => {
            handle_mouse_drag(model, mouse);
            Vec::new()
        }
        MouseEventKind::Up(MouseButton::Left) if model.drag.is_some() => {
            model.drag = None;
            Vec::new()
        }
        MouseEventKind::ScrollUp => handle_mouse_scroll(model, mouse, -1),
        MouseEventKind::ScrollDown => handle_mouse_scroll(model, mouse, 1),
        MouseEventKind::ScrollLeft => handle_mouse_horizontal_scroll(model, Action::ResultsLeft),
        MouseEventKind::ScrollRight => handle_mouse_horizontal_scroll(model, Action::ResultsRight),
        _ => Vec::new(),
    }
}

fn handle_mouse_down(model: &mut Model, mouse: MouseEvent) -> Vec<Effect> {
    let hit = model.hits.at(mouse.column, mouse.row);
    let doubled = hit.map(|target| note_click(model, target)).unwrap_or(false);
    match top_overlay(model) {
        Some(OverlayKind::Onboarding) => mouse_onboarding(model, hit),
        Some(OverlayKind::Palette) => mouse_palette(model, hit),
        Some(OverlayKind::Help) => mouse_help(model, hit),
        Some(OverlayKind::ClosePrompt) => mouse_close_prompt(model, hit),
        Some(OverlayKind::DeleteConnection) => mouse_delete_connection(model, hit),
        Some(OverlayKind::NodeMenu) => mouse_node_menu(model, hit),
        Some(OverlayKind::ResultsMenu) => mouse_results_menu(model, hit),
        Some(OverlayKind::Review) => mouse_review(model, hit),
        Some(OverlayKind::DdlPreview) => mouse_ddl_preview(model, hit),
        Some(OverlayKind::SchemaDiff) => mouse_schema_diff(model, hit),
        Some(OverlayKind::Transfer) => mouse_transfer(model, hit),
        Some(OverlayKind::Security) => mouse_security(model, hit, doubled),
        Some(OverlayKind::Admin) => mouse_admin(model, hit),
        Some(OverlayKind::McpProfiles) => mouse_mcp_profiles(model, hit),
        Some(OverlayKind::ObjectOverlay) => {
            model.inspector.open = false;
            Vec::new()
        }
        Some(OverlayKind::SchemaForm) => {
            model.schema_editor.open = false;
            Vec::new()
        }
        Some(OverlayKind::ValueViewer) => {
            model.data.viewer = None;
            Vec::new()
        }
        Some(OverlayKind::Connections) => mouse_connections(model, hit, doubled),
        Some(OverlayKind::Projects) => mouse_projects(model, hit, doubled),
        Some(OverlayKind::ConfigTransfer) => mouse_config_transfer(model, hit),
        Some(OverlayKind::SecretPrompt) => mouse_secret(model, hit),
        Some(OverlayKind::TransactionPrompt) => mouse_transaction(model, hit),
        Some(OverlayKind::DocumentNamePrompt) => mouse_document_name(model, hit),
        Some(OverlayKind::DataQueryPrompt) => mouse_data_query(model, hit),
        Some(OverlayKind::ConnectionForm) => mouse_connection_form(model, hit),
        Some(OverlayKind::Settings) => mouse_settings(model, hit),
        Some(OverlayKind::Recovery) => mouse_recovery(model, hit),
        Some(OverlayKind::Diagnostics) => mouse_diagnostics(model, hit),
        Some(OverlayKind::McpAudit) => mouse_mcp_audit(model, hit),
        Some(OverlayKind::FilePicker) => mouse_file_picker(model, hit, doubled),
        // A click away from the popup dismisses it and still lands where it was aimed,
        // the way clicking elsewhere in a code editor does.
        Some(OverlayKind::Completion) => {
            if let Some(HitTarget::ListRow(index)) = hit {
                model.editor.completion_selected = index;
                update(model, Action::AcceptCompletion)
            } else {
                crate::screens::editor::close_completion(model);
                mouse_workbench(model, mouse, hit, doubled)
            }
        }
        Some(OverlayKind::Parameters) => mouse_parameters(model, hit),
        Some(OverlayKind::History) => mouse_history(model, hit),
        Some(OverlayKind::Snippets) => mouse_snippets(model, hit),
        None => mouse_workbench(model, mouse, hit, doubled),
    }
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
        model.help.query.clear();
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

fn mouse_document_name(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FormField(_)) => {
            model.document_name_prompt.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => submit_document_name_prompt(model),
        Some(HitTarget::FooterCancel) => {
            model.document_name_prompt.open = false;
            model.document_name_prompt.error = None;
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
fn mouse_onboarding(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    if matches!(hit, Some(HitTarget::Button(HitButton::GetStarted))) {
        return complete_onboarding(model);
    }
    Vec::new()
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
    let rows = file_picker_rows(model);
    match hit {
        Some(HitTarget::RecentSqlFile(index)) => {
            model.file_picker.select_recent(index);
            if doubled {
                file_picker_submit(model)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::ListRow(index)) => {
            model.file_picker.select_browser(index, rows);
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
        Some(HitTarget::FooterCancel) => cancel_file_picker(model),
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
        Some(HitTarget::FormField(_)) => Vec::new(),
        Some(HitTarget::FooterSubmit) => update(model, Action::SubmitParameters),
        Some(HitTarget::FooterCancel) => {
            crate::screens::editor::cancel_parameters(model);
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
        Some(HitTarget::Button(HitButton::Export)) if !model.diagnostics.writing => {
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
            if model.mcp_profiles.revoke_all() {
                vec![Effect::RevokeAllMcpGrants]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn mouse_settings(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) if index < crate::screens::settings::FIELD_COUNT => {
            model.settings.focus = index;
            model.settings.confirm_reset = false;
            step_focused_setting(model, 1)
        }
        Some(HitTarget::Button(HitButton::Reset)) => {
            model.settings.focus = crate::screens::settings::RESET_FOCUS;
            update(model, Action::ConfirmResetSettings)
        }
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
        Some(HitTarget::ResultTab(index)) => update(model, Action::SelectResultTab { index }),
        Some(HitTarget::ResultsView(index)) => {
            if let Some(view) = crate::model::ResultsView::ALL.get(index).copied() {
                model.results.view = view;
                model.results.explain_scroll = 0;
                model.results.messages_scroll = 0;
            }
            model.focus = Focus::Results;
            Vec::new()
        }
        Some(HitTarget::DocumentTab(index)) => update(model, Action::SelectDocument { index }),
        Some(HitTarget::DocumentTabClose(index)) => {
            let mut effects = update(model, Action::SelectDocument { index });
            effects.extend(update(model, Action::CloseDocument));
            effects
        }
        Some(HitTarget::DocumentTabNew) => update(model, Action::NewDocument),
        Some(HitTarget::DocumentTabScrollPrev) => update(model, Action::ScrollDocumentTabsPrev),
        Some(HitTarget::DocumentTabScrollNext) => update(model, Action::ScrollDocumentTabsNext),
        Some(HitTarget::Button(HitButton::New)) => update(model, Action::OpenConnectionForm),
        Some(HitTarget::Button(HitButton::Edit)) => update(model, Action::EditSelectedConnection),
        Some(HitTarget::Button(HitButton::Actions)) => update(model, Action::OpenNodeMenu),
        Some(HitTarget::PaneDivider(edge))
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
        {
            start_pane_drag(model, edge, mouse);
            Vec::new()
        }
        Some(HitTarget::Explorer) => update(model, Action::Focus(FocusTarget::Explorer)),
        Some(HitTarget::ExplorerNode(index)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Explorer;
            model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
            if index < model.explorer.visible_ids().len() {
                model.explorer.select_visible(index);
                if let Some(profile_index) = selected_connection_profile_index(model) {
                    model.connections.selected_profile = profile_index;
                }
            }
            let activate = doubled
                || model
                    .explorer
                    .selected_node()
                    .is_some_and(crate::screens::explorer::is_connection_node);
            if activate {
                activate_connection_or_catalog(model)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::SidebarConnection(index)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Explorer;
            model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
            if index < model.connections.profiles.len() {
                let name = model.connections.profiles[index].profile.name.clone();
                model.connections.selected_profile = index;
                model
                    .explorer
                    .select(crate::screens::explorer::connection_id(&name));
            }
            activate_connection_or_catalog(model)
        }
        Some(HitTarget::Editor) => {
            let effects = update(model, Action::Focus(FocusTarget::Editor));
            let plan = LayoutPlan::for_area_with_document_tabs(
                Rect::new(0, 0, model.width, model.height),
                Some(&model.effective_panes()),
                true,
            );
            if let Some(index) =
                crate::widgets::editor::char_index_at(model, plan.content, mouse.column, mouse.row)
            {
                let doc = model.active_document_mut();
                doc.anchor = None;
                let _ = doc.sql.set_cursor(index);
                start_editor_selection_drag(model, index, mouse);
            }
            effects
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
            if doubled {
                update(model, Action::OpenResultsMenu)
            } else if pick {
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
            if doubled {
                update(model, Action::OpenResultsMenu)
            } else if pick {
                update(model, Action::ToggleResultsPick)
            } else {
                Vec::new()
            }
        }
        Some(HitTarget::Grid) => {
            // `FocusTarget` names a position, and a table document puts the grid in the
            // editor's.
            let target = if model.active_document().kind.is_table() {
                FocusTarget::Editor
            } else {
                FocusTarget::Results
            };
            update(model, Action::Focus(target))
        }
        // Only a table document gives the console a pane of its own, in pane 3's place.
        Some(HitTarget::Console) => update(model, Action::Focus(FocusTarget::Results)),
        _ => Vec::new(),
    }
}

fn start_pane_drag(model: &mut Model, edge: PaneEdge, mouse: MouseEvent) {
    let start_value = match edge {
        PaneEdge::Explorer => model.panes.explorer_width,
        PaneEdge::Results => bottom_pane_height(model),
    };
    model.drag = Some(DragState {
        kind: DragKind::PaneDivider(edge),
        origin_x: mouse.column,
        origin_y: mouse.row,
        start_value,
    });
}

fn start_editor_selection_drag(model: &mut Model, anchor: usize, mouse: MouseEvent) {
    model.drag = Some(DragState {
        kind: DragKind::EditorSelect { anchor },
        origin_x: mouse.column,
        origin_y: mouse.row,
        start_value: 0,
    });
}

/// Right-click opens the context menu of whatever it lands on. Nothing here is
/// reachable only this way: the sidebar menu is also `a`, the grid menu also Enter.
fn handle_mouse_right_down(model: &mut Model, mouse: MouseEvent) -> Vec<Effect> {
    if crate::mouse::overlay_blocks_workbench(model) {
        return Vec::new();
    }
    match model.hits.at(mouse.column, mouse.row) {
        Some(HitTarget::ExplorerNode(index)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Explorer;
            model.explorer.sidebar_focus = crate::screens::explorer::SidebarFocus::Catalog;
            if index < model.explorer.visible_ids().len() {
                model.explorer.select_visible(index);
                if let Some(profile_index) = selected_connection_profile_index(model) {
                    model.connections.selected_profile = profile_index;
                }
            }
            update(model, Action::OpenNodeMenu)
        }
        Some(HitTarget::GridRow(row)) | Some(HitTarget::GridCell { row, .. }) => {
            model.focus = Focus::Results;
            click_results_row(model, row, false);
            update(model, Action::OpenResultsMenu)
        }
        _ => Vec::new(),
    }
}

fn handle_mouse_drag(model: &mut Model, mouse: MouseEvent) {
    match model.drag.map(|drag| drag.kind) {
        Some(DragKind::PaneDivider(_)) => resize_pane_drag(model, mouse),
        Some(DragKind::EditorSelect { anchor }) => extend_editor_selection(model, anchor, mouse),
        None => {}
    }
}

fn extend_editor_selection(model: &mut Model, anchor: usize, mouse: MouseEvent) {
    let plan = LayoutPlan::for_area_with_document_tabs(
        Rect::new(0, 0, model.width, model.height),
        Some(&model.effective_panes()),
        true,
    );
    if let Some(index) =
        crate::widgets::editor::char_index_at(model, plan.content, mouse.column, mouse.row)
    {
        model.active_document_mut().anchor = Some(anchor);
        crate::screens::editor::extend_selection_to(model, index);
    }
}

fn resize_pane_drag(model: &mut Model, mouse: MouseEvent) {
    let Some(DragState {
        kind: DragKind::PaneDivider(edge),
        origin_x,
        origin_y,
        start_value,
    }) = model.drag
    else {
        return;
    };
    let delta = match edge {
        PaneEdge::Explorer => i32::from(mouse.column) - i32::from(origin_x),
        PaneEdge::Results => i32::from(origin_y) - i32::from(mouse.row),
    };
    let value = (i32::from(start_value) + delta).clamp(0, i32::from(u16::MAX)) as u16;
    match edge {
        PaneEdge::Explorer => model.panes.explorer_width = value,
        PaneEdge::Results => set_bottom_pane_height(model, value),
    }
    model.panes = model.panes.clamp(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn handle_mouse_scroll(model: &mut Model, mouse: MouseEvent, delta: i32) -> Vec<Effect> {
    let overlay = top_overlay(model);
    if overlay == Some(OverlayKind::Palette) {
        move_palette_selection(model, delta as isize);
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Help) {
        if delta < 0 {
            model.help.scroll = model.help.scroll.saturating_sub(1);
        } else {
            model.help.scroll = model.help.scroll.saturating_add(1);
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::ResultsMenu) {
        let area = Rect::new(0, 0, model.width, model.height);
        let layout = crate::render::results_menu_layout(area);
        let row = model.results.cursor_row().unwrap_or(0);
        let wrap_width = layout.detail.width.max(1) as usize;
        let detail_fields = crate::widgets::row_detail::row_detail_fields(&model.results, row);
        let detail_lines = crate::widgets::row_detail::row_detail_lines(
            &detail_fields,
            wrap_width,
            &model.theme,
            model.capabilities,
        );
        let detail_rows = layout.detail.height.max(1) as usize;
        let max_detail_offset = detail_lines.len().saturating_sub(detail_rows);
        if layout
            .detail
            .contains(Position::new(mouse.column, mouse.row))
        {
            if delta < 0 {
                model.results_menu.offset = model.results_menu.offset.saturating_sub(1);
            } else {
                model.results_menu.offset = (model.results_menu.offset + 1).min(max_detail_offset);
            }
        } else {
            let count = crate::palette::results_menu_items().len();
            if count > 0 {
                if delta < 0 {
                    model.results_menu.selected = model.results_menu.selected.saturating_sub(1);
                } else {
                    model.results_menu.selected =
                        (model.results_menu.selected + 1).min(count.saturating_sub(1));
                }
            }
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::FilePicker) {
        model
            .file_picker
            .move_selection(delta, file_picker_rows(model));
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Completion) {
        if matches!(
            model.hits.at(mouse.column, mouse.row),
            Some(HitTarget::ListRow(_))
        ) {
            crate::screens::editor::move_completion(model, delta);
            return Vec::new();
        }
        // Scrolling the text means reading elsewhere; the popup would float over the
        // wrong line.
        crate::screens::editor::close_completion(model);
    }
    if overlay == Some(OverlayKind::History) {
        if delta < 0 {
            model.editor.history_selected = model.editor.history_selected.saturating_sub(1);
        } else if !model.editor.history.is_empty() {
            model.editor.history_selected = (model.editor.history_selected + 1)
                .min(model.editor.history.len().saturating_sub(1));
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Snippets) {
        if delta < 0 {
            model.editor.snippet_selected = model.editor.snippet_selected.saturating_sub(1);
        } else if !model.editor.snippets.is_empty() {
            model.editor.snippet_selected = (model.editor.snippet_selected + 1)
                .min(model.editor.snippets.len().saturating_sub(1));
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::ConnectionForm) {
        if delta < 0 {
            model.connection_form.focus_prev();
        } else {
            model.connection_form.focus_next();
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::SchemaDiff) {
        let count = model.schema_diff.filtered().len();
        if count > 0 {
            if delta < 0 {
                model.schema_diff.selected = model.schema_diff.selected.saturating_sub(1);
            } else {
                model.schema_diff.selected = (model.schema_diff.selected + 1).min(count - 1);
            }
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Security) {
        if delta < 0 {
            model.security.select_previous();
        } else {
            model.security.select_next();
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Transfer) {
        model.transfer.scroll = if delta < 0 {
            model.transfer.scroll.saturating_sub(1)
        } else {
            model
                .transfer
                .scroll
                .saturating_add(1)
                .min(model.transfer.lines().len().saturating_sub(1))
        };
        return Vec::new();
    }
    if overlay == Some(OverlayKind::McpProfiles) {
        if delta < 0 {
            model.mcp_profiles.select_previous();
        } else {
            model.mcp_profiles.select_next();
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Connections) {
        if delta < 0 {
            model.connections.selected_profile =
                model.connections.selected_profile.saturating_sub(1);
        } else if model.connections.selected_profile + 1 < model.connections.profiles.len() {
            model.connections.selected_profile += 1;
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Projects) {
        if delta < 0 {
            model.projects.selected = model.projects.selected.saturating_sub(1);
        } else if model.projects.selected + 1 < model.projects.list.len() {
            model.projects.selected += 1;
        }
        return Vec::new();
    }
    if overlay.is_some() {
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
        _ => match model.effective_focus() {
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
            // The wheel over the strip walks it, the way it walks every other pane.
            Focus::DocumentTabs => update(model, Action::MoveDocumentTabCursor(delta)),
            Focus::Console => Vec::new(),
            Focus::Editor | Focus::Palette => {
                let doc = model.active_document_mut();
                if delta < 0 {
                    doc.viewport_line = doc.viewport_line.saturating_sub(1);
                } else {
                    doc.viewport_line = doc.viewport_line.saturating_add(1);
                }
                Vec::new()
            }
        },
    }
}

fn handle_mouse_horizontal_scroll(model: &mut Model, action: Action) -> Vec<Effect> {
    if top_overlay(model).is_some() {
        Vec::new()
    } else {
        update(model, action)
    }
}

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if model.onboarding.open {
        return handle_onboarding_key(model, key);
    }
    if key.kind != KeyEventKind::Press {
        return Vec::new();
    }
    // Esc clears the toast without consuming the key: every overlay below still gets its
    // Esc, and the toast never becomes one more thing standing between you and a close.
    if key.code == KeyCode::Esc {
        model.messages.dismiss();
    }
    if model.palette.open {
        return handle_palette_key(model, key);
    }
    if model.help.open {
        return handle_help_key(model, key);
    }
    if model.close_prompt.is_some() {
        return handle_close_prompt_key(model, key);
    }
    if model.connections.delete_target.is_some() {
        return handle_delete_connection_key(model, key);
    }
    if model.node_menu.open {
        return handle_node_menu_key(model, key);
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
    if model.document_name_prompt.open {
        return handle_document_name_prompt_key(model, key);
    }
    if model.data.query_prompt.open {
        return handle_data_query_prompt_key(model, key);
    }
    if model.schema_editor.open {
        return match key.code {
            KeyCode::Esc => {
                model.schema_editor.open = false;
                Vec::new()
            }
            KeyCode::Tab => {
                model.schema_editor.focus_next();
                Vec::new()
            }
            KeyCode::Enter => update(model, Action::OpenDdlPreview),
            _ => Vec::new(),
        };
    }
    if model.inspector.open {
        return match key.code {
            KeyCode::Esc => {
                model.inspector.open = false;
                Vec::new()
            }
            KeyCode::Up => {
                model.inspector.scroll = model.inspector.scroll.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Down => {
                model.inspector.scroll = model.inspector.scroll.saturating_add(1);
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.data.viewer.is_some() {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            model.data.viewer = None;
        }
        return Vec::new();
    }
    if model.file_picker.open {
        return handle_file_picker_key(model, key);
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
    if model.editor.history_open {
        return handle_history_overlay(model, key);
    }
    if model.editor.snippet_open {
        crate::screens::editor::handle_snippet_key(model, key);
        return Vec::new();
    }
    if model.editor.parameter_prompt {
        let outcome = crate::screens::editor::handle_parameter_key(model, key);
        // Only a submit that answered the last parameter runs the statement. A cancel
        // closed the prompt the same way and this could not tell them apart, so Esc ran
        // the query -- which found the parameter still null and asked for it again, and
        // looked like a key that did nothing.
        if outcome == crate::widgets::form::FooterKey::Submit && !model.editor.parameter_prompt {
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
        use crate::widgets::form::{FooterKey, footer_key};
        match footer_key(&mut model.transfer.footer, &key) {
            FooterKey::Cancel => {
                model.transfer.open = false;
                return Vec::new();
            }
            FooterKey::Submit => return run_transfer(model),
            FooterKey::Moved => return Vec::new(),
            FooterKey::Pass => {}
        }
        return match key.code {
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
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                model.transfer.format = next_transfer_format(&model.transfer.format);
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
    if model.data.insert_form.open {
        return match key.code {
            KeyCode::Esc => update(model, Action::CancelInsertRow),
            KeyCode::Enter => update(model, Action::SubmitInsertRow),
            KeyCode::Up => {
                model.data.insert_form.focus = model
                    .data
                    .insert_form
                    .focus
                    .checked_sub(1)
                    .unwrap_or(model.data.insert_form.fields.len().saturating_sub(1));
                Vec::new()
            }
            KeyCode::Down | KeyCode::Tab => {
                model.data.insert_form.focus =
                    (model.data.insert_form.focus + 1) % model.data.insert_form.fields.len().max(1);
                Vec::new()
            }
            KeyCode::Backspace => {
                if let Some(field) = model
                    .data
                    .insert_form
                    .fields
                    .get_mut(model.data.insert_form.focus)
                {
                    field.value.pop();
                }
                Vec::new()
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(field) = model
                    .data
                    .insert_form
                    .fields
                    .get_mut(model.data.insert_form.focus)
                {
                    field.value.push(ch);
                }
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
                model.mcp_profiles.confirm_revoke = None;
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
            KeyCode::Enter => match model.mcp_profiles.confirm_pending_revoke() {
                Some(crate::screens::mcp_profiles::RevokeScope::Profile(profile)) => {
                    vec![Effect::RevokeMcpGrants { profile }]
                }
                Some(crate::screens::mcp_profiles::RevokeScope::All) => {
                    vec![Effect::RevokeAllMcpGrants]
                }
                None => Vec::new(),
            },
            KeyCode::Char('e') => update(model, Action::ToggleMcpProfile),
            KeyCode::Char('r') => update(model, Action::RevokeProfileGrants),
            KeyCode::Char('R') => {
                if model.mcp_profiles.revoke_all() {
                    vec![Effect::RevokeAllMcpGrants]
                } else {
                    Vec::new()
                }
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
            KeyCode::Up | KeyCode::BackTab => {
                model.settings.focus_prev();
                Vec::new()
            }
            KeyCode::Down | KeyCode::Tab => {
                model.settings.focus_next();
                Vec::new()
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => step_focused_setting(model, 1),
            KeyCode::Left => step_focused_setting(model, -1),
            KeyCode::Char('r') => update(model, Action::ConfirmResetSettings),
            KeyCode::Char('t') => update(model, Action::CycleMode),
            KeyCode::Char('c') => update(model, Action::CycleAccent),
            KeyCode::Char('k') => update(model, Action::CycleKeymap),
            KeyCode::Char('m') => update(model, Action::ToggleMouse),
            KeyCode::Char('a') => update(model, Action::ToggleAnimation),
            KeyCode::Char('u') => update(model, Action::ToggleUnicode),
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
            model.messages.error(format!(
                "keymap conflict {}: {}",
                conflict.chord,
                conflict.commands.join(" / ")
            ));
            return Vec::new();
        }
    }
    let revision = model.active_document().sql.revision();
    if !model.active_document().kind.is_table() && crate::screens::editor::handle_key(model, key) {
        // Highlighting and parameters are functions of the text. An arrow key moves the
        // cursor and changes neither, and re-deriving them from the whole buffer on every
        // repeat was half of what made a long script lag behind the key.
        if model.active_document().sql.revision() != revision {
            crate::screens::editor::refresh_intelligence(model, false);
        }
        return crate::screens::editor::take_completion_effects(model);
    }
    Vec::new()
}

fn active_key_context(model: &Model) -> crate::keymap::KeyContext {
    use crate::keymap::KeyContext;
    if model.palette.open {
        return KeyContext::Palette;
    }
    match model.effective_focus() {
        Focus::Explorer => KeyContext::Explorer,
        Focus::Results => KeyContext::Results,
        // Nothing in the console is navigable, so its context holds only the keys that
        // act on the pane itself. Global chords -- the way back out included -- still
        // resolve there.
        Focus::Console => KeyContext::Console,
        Focus::DocumentTabs => KeyContext::DocumentTabs,
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

/// The "Delete connection" dialog: arrows and Tab move between its buttons, Enter
/// presses the focused one, Esc cancels. No letter deletes -- `d` duplicates in the list
/// under it, and used to delete here.
fn handle_delete_connection_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::connections::DeleteChoice;
    match key.code {
        KeyCode::Esc => resolve_delete_connection(model, DeleteChoice::Cancel),
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Tab
        | KeyCode::BackTab => {
            model.connections.delete_choice = model.connections.delete_choice.toggle();
            Vec::new()
        }
        KeyCode::Enter => {
            let choice = model.connections.delete_choice;
            resolve_delete_connection(model, choice)
        }
        _ => Vec::new(),
    }
}

fn mouse_delete_connection(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    use crate::screens::connections::DeleteChoice;
    match hit {
        Some(HitTarget::Button(HitButton::ConfirmDelete)) => {
            resolve_delete_connection(model, DeleteChoice::Delete)
        }
        Some(HitTarget::Button(HitButton::Cancel)) => {
            resolve_delete_connection(model, DeleteChoice::Cancel)
        }
        _ => Vec::new(),
    }
}

/// The saved password goes with the connection: a duplicate gets passwords of its own,
/// so nothing else can be using it.
fn resolve_delete_connection(
    model: &mut Model,
    choice: crate::screens::connections::DeleteChoice,
) -> Vec<Effect> {
    match choice {
        crate::screens::connections::DeleteChoice::Cancel => {
            model.connections.ask_delete(None);
            Vec::new()
        }
        crate::screens::connections::DeleteChoice::Delete => update(
            model,
            Action::ConfirmDeleteProfile {
                decision: crate::screens::secret_prompt::DeleteSecretDecision::DeleteSecrets,
            },
        ),
    }
}

fn handle_connections_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            model.connections.open = false;
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

fn explorer_sidebar_header_rows(model: &Model) -> usize {
    if model.connections.profiles.is_empty() {
        2
    } else {
        1
    }
}

fn explorer_visible_rows(model: &Model) -> usize {
    let area = Rect::new(0, 0, model.width.max(1), model.height.max(1));
    let plan = LayoutPlan::for_area_with(area, Some(&model.effective_panes()));
    let height = if matches!(plan.mode, crate::layout::LayoutMode::Compact) {
        plan.content.height
    } else {
        plan.explorer.height
    };
    height
        .saturating_sub(2)
        .saturating_sub(explorer_sidebar_header_rows(model) as u16)
        .max(1) as usize
}

fn sync_explorer_connections(model: &mut Model) {
    model
        .explorer
        .sync_connection_roots(&model.connections.profiles, model.connection.name.as_str());
}

fn selected_connection_profile_index(model: &Model) -> Option<usize> {
    let name = model.explorer.selected_connection_name()?;
    model
        .connections
        .profiles
        .iter()
        .position(|row| row.profile.name == name)
}

fn close_selected_session(model: &mut Model) -> Vec<Effect> {
    let connection_name = if model.connections.open {
        model
            .connections
            .selected()
            .map(|profile| profile.name.clone())
    } else if model.focus == Focus::Explorer {
        model.explorer.selected_connection_name().map(str::to_owned)
    } else {
        model.active_session.and_then(|active| {
            model
                .connections
                .sessions
                .iter()
                .find(|session| session.id == active)
                .map(|session| session.connection.clone())
        })
    };

    let Some(connection_name) = connection_name else {
        model
            .messages
            .warn("select a connection to disconnect".into());
        return Vec::new();
    };

    if let Some(index) = model
        .connections
        .profiles
        .iter()
        .position(|row| row.profile.name == connection_name)
    {
        model.connections.selected_profile = index;
    }

    let Some(session) = model
        .connections
        .session_for(&connection_name)
        .map(|session| session.id)
    else {
        model
            .messages
            .warn(format!("{connection_name} is disconnected"));
        return Vec::new();
    };

    vec![Effect::CloseSession { session }]
}

fn activate_connection_or_catalog(model: &mut Model) -> Vec<Effect> {
    if model
        .explorer
        .selected_node()
        .is_some_and(crate::screens::explorer::is_connection_node)
    {
        activate_connection_node(model)
    } else {
        expand_or_open_selected(model)
    }
}

fn activate_connection_node(model: &mut Model) -> Vec<Effect> {
    let Some(index) = selected_connection_profile_index(model) else {
        return Vec::new();
    };
    model.connections.selected_profile = index;
    let name = model.connections.profiles[index].profile.name.clone();
    let profile = model.connections.profiles[index].profile.clone();
    let connection = crate::screens::explorer::connection_id(&name);
    if model.connections.session_for(&name).is_none() {
        return connect_selected(model);
    }
    if let Some(session) = model.connections.session_for(&name).cloned()
        && model.active_session != Some(session.id)
    {
        return activate_existing_session(model, &profile, session);
    }
    if let Some(node) = model.explorer.selected_node() {
        if node.expanded {
            if crate::screens::explorer::is_connection_node(node) && node.children.is_empty() {
                if matches!(node.state, crate::screens::explorer::NodeState::Loading(_)) {
                    return Vec::new();
                }
                return expand_selected_catalog(model);
            }
            model.explorer.collapse(&connection);
            return Vec::new();
        }
        if !node.children.is_empty() {
            model.explorer.expand_local(&connection);
            return Vec::new();
        }
    }
    expand_selected_catalog(model)
}

fn connect_selected(model: &mut Model) -> Vec<Effect> {
    let Some(profile) = model.connections.selected().cloned() else {
        return Vec::new();
    };
    if let Some(session) = model.connections.session_for(&profile.name).cloned() {
        return activate_existing_session(model, &profile, session);
    }
    connect_to(model, profile)
}

fn connect_to(model: &mut Model, profile: dexo_app::ConnectionProfile) -> Vec<Effect> {
    model.connect_token = model.connect_token.saturating_add(1);
    model.connections.pending_connect = Some(model.connect_token);
    // The dial is spawned, so this toast paints on the very next frame and is the only
    // thing telling the user the Enter landed at all.
    model
        .messages
        .info(format!("Connecting to {}…", profile.name));
    vec![Effect::ConnectProfile {
        profile,
        token: model.connect_token,
    }]
}

fn move_sidebar_selection(model: &mut Model, delta: i32) {
    model.explorer.move_selection(delta);
    model.explorer.sync_scroll(explorer_visible_rows(model));
    if let Some(index) = selected_connection_profile_index(model) {
        model.connections.selected_profile = index;
    }
}

fn enter_offline_explorer(model: &mut Model) -> Vec<Effect> {
    sync_explorer_connections(model);
    model.explorer.offline = true;
    if model.connection.name.is_empty() {
        return Vec::new();
    }
    model
        .explorer
        .select(crate::screens::explorer::connection_id(
            &model.connection.name,
        ));
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
    sync_explorer_connections(model);
    let connection = crate::screens::explorer::connection_id(&profile.name);
    model.explorer.select(connection.clone());
    let operation = crate::runtime::OperationId::new();
    model.explorer.expand_with(&connection, operation);
    vec![Effect::LoadCatalogChildren {
        parent: Some(connection),
        operation,
        session: session.id,
        generation: session.generation,
        replace_roots: false,
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
    use crate::widgets::form::{FooterKey, footer_key};
    match footer_key(&mut model.transaction_prompt.footer, &key) {
        FooterKey::Cancel => {
            model.transaction_prompt.open = false;
            model.transaction_prompt.error = None;
            return Vec::new();
        }
        FooterKey::Submit => return submit_savepoint_prompt(model),
        FooterKey::Moved => return Vec::new(),
        FooterKey::Pass => {}
    }
    match key.code {
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

fn handle_document_name_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, footer_key};
    match footer_key(&mut model.document_name_prompt.footer, &key) {
        FooterKey::Cancel => {
            model.document_name_prompt.open = false;
            model.document_name_prompt.error = None;
            return Vec::new();
        }
        FooterKey::Submit => return submit_document_name_prompt(model),
        FooterKey::Moved => return Vec::new(),
        FooterKey::Pass => {}
    }
    match key.code {
        KeyCode::Char(_)
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Backspace
        | KeyCode::Delete
            if model.document_name_prompt.footer == crate::widgets::form::FooterFocus::Input =>
        {
            let _ = model.document_name_prompt.name.handle_key(key);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_data_query_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, footer_key};
    // Tab already belongs to this prompt -- it flips the sort direction, or moves
    // between the filter's column and value -- so it is answered before the footer is.
    if key.code != KeyCode::Tab {
        match footer_key(&mut model.data.query_prompt.footer, &key) {
            FooterKey::Cancel => {
                model.data.query_prompt.open = false;
                model.data.query_prompt.error = None;
                return Vec::new();
            }
            FooterKey::Submit => return submit_data_query_prompt(model),
            FooterKey::Moved => return Vec::new(),
            FooterKey::Pass => {}
        }
    }
    match key.code {
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
        crate::palette::popup_list_rows(model.height, count),
    );
}
fn complete_onboarding(model: &mut Model) -> Vec<Effect> {
    model.onboarding.open = false;
    vec![Effect::CompleteOnboarding]
}

fn handle_onboarding_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => complete_onboarding(model),
        KeyCode::F(1) => {
            let effects = complete_onboarding(model);
            toggle_help(model);
            effects
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let effects = complete_onboarding(model);
            open_palette(model);
            effects
        }
        _ => Vec::new(),
    }
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
        model.help.query.clear();
        return;
    }
    close_palette(model);
    model.results_menu.open = false;
    model.help.open = true;
    model.help.scroll = 0;
    model.help.query.clear();
}

fn handle_help_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc | KeyCode::F(1) => {
            model.help.open = false;
            model.help.scroll = 0;
            model.help.query.clear();
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
        KeyCode::Backspace => {
            model.help.query.pop();
            model.help.scroll = 0;
            Vec::new()
        }
        KeyCode::Char(ch) => {
            model.help.query.push(ch);
            model.help.scroll = 0;
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

/// Which set of commands the selected node offers, or `None` when nothing is selected.
pub fn node_menu_kind(model: &Model) -> Option<crate::palette::NodeMenuKind> {
    use crate::palette::NodeMenuKind;
    let node = model.explorer.selected_node()?;
    Some(if crate::screens::explorer::is_connection_node(node) {
        NodeMenuKind::Connection
    } else if crate::screens::explorer::opens_table_data(&node.kind) {
        NodeMenuKind::Relation
    } else {
        NodeMenuKind::Object
    })
}

fn open_node_menu(model: &mut Model) {
    if node_menu_kind(model).is_none() {
        model
            .messages
            .warn("Select an object in the sidebar first.".into());
        return;
    }
    model.node_menu.open = true;
    model.node_menu.selected = 0;
    model.node_menu.offset = 0;
}

fn node_menu_entries(model: &Model) -> Vec<crate::palette::PaletteEntry> {
    node_menu_kind(model)
        .map(|kind| crate::palette::node_menu_entries(model, kind))
        .unwrap_or_default()
}

fn handle_node_menu_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let count = node_menu_entries(model).len();
    match key.code {
        KeyCode::Esc => {
            model.node_menu.open = false;
            Vec::new()
        }
        KeyCode::Up => {
            model.node_menu.selected = model.node_menu.selected.saturating_sub(1);
            Vec::new()
        }
        KeyCode::Down => {
            model.node_menu.selected = (model.node_menu.selected + 1).min(count.saturating_sub(1));
            Vec::new()
        }
        KeyCode::Enter => pick_node_menu(model),
        _ => Vec::new(),
    }
}

fn pick_node_menu(model: &mut Model) -> Vec<Effect> {
    let entries = node_menu_entries(model);
    model.node_menu.open = false;
    let Some(entry) = entries.get(model.node_menu.selected) else {
        return Vec::new();
    };
    // A row the palette would refuse is refused here too, with the same sentence.
    if let Some(reason) = &entry.disabled_reason {
        model.messages.warn(format!("{}: {reason}", entry.title));
        return Vec::new();
    }
    invoke_palette(model, entry.invocation.clone())
}

fn mouse_node_menu(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.node_menu.selected = index;
            pick_node_menu(model)
        }
        Some(HitTarget::Overlay) => Vec::new(),
        _ => {
            model.node_menu.open = false;
            Vec::new()
        }
    }
}

fn handle_results_menu_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    let count = crate::palette::results_menu_items().len();
    let area = Rect::new(0, 0, model.width, model.height);
    let layout = crate::render::results_menu_layout(area);
    let row = model.results.cursor_row().unwrap_or(0);
    let wrap_width = layout.detail.width.max(1) as usize;
    let detail_fields = crate::widgets::row_detail::row_detail_fields(&model.results, row);
    let detail_lines = crate::widgets::row_detail::row_detail_lines(
        &detail_fields,
        wrap_width,
        &model.theme,
        model.capabilities,
    );
    let detail_rows = layout.detail.height.max(1) as usize;
    let max_detail_offset = detail_lines.len().saturating_sub(detail_rows);
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
        KeyCode::PageUp => {
            model.results_menu.offset = model.results_menu.offset.saturating_sub(detail_rows);
            Vec::new()
        }
        KeyCode::PageDown => {
            model.results_menu.offset =
                (model.results_menu.offset + detail_rows).min(max_detail_offset);
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

/// The bottom pane is the console on a table document and the result grid everywhere
/// else, and the two keep separate heights -- see `PaneLayout::console_height`.
fn bottom_pane_height(model: &Model) -> u16 {
    if model.active_document().kind.is_table() {
        model.panes.console_height
    } else {
        model.panes.results_height
    }
}

fn set_bottom_pane_height(model: &mut Model, value: u16) {
    if model.active_document().kind.is_table() {
        model.panes.console_height = value;
    } else {
        model.panes.results_height = value;
    }
}

fn adjust_results_height(model: &mut Model, delta: i16) {
    let next = (bottom_pane_height(model) as i16 + delta).max(3) as u16;
    model.panes.results_visible = true;
    set_bottom_pane_height(model, next);
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
fn flush_documents_effect(model: &Model) -> Effect {
    Effect::FlushDocuments {
        project_id: model.project_id.clone(),
        documents: model
            .documents
            .iter()
            .filter(|document| !document.kind.is_placeholder())
            .map(|document| crate::action::FlushedDocument {
                kind: document.kind.storage_tag(),
                connection_id: document.connection_id.clone(),
                id: document.id.clone(),
                title: document.title.clone(),
                content: document.text(),
                path: document.path.clone(),
            })
            .collect(),
    }
}

fn checkpoint_session(model: &Model) -> Vec<Effect> {
    let mut effects = checkpoint_dirty(model);
    if model.layout_dirty && !model.project_id.is_empty() {
        effects.push(persist_layout_effect(model));
    }
    effects
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

fn profile_by_uuid(model: &Model, id: &str) -> Option<dexo_app::ConnectionProfile> {
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.id.0.to_string() == id)
        .map(|row| row.profile.clone())
}

/// What `switch_to_document_connection` did. The three cases read differently to a
/// caller that wants to execute: only `Dialling` means the query has to wait, and
/// collapsing them into an `Option` hid that -- dialling another connection leaves the
/// old session live, so "no session yet" is not the test for whether to wait.
enum Switch {
    /// The live session already belongs to the document.
    Ready,
    /// Moved onto a session that was already open. Whatever is next can proceed.
    Activated(Vec<Effect>),
    /// The connection is being dialled. Nothing may run against it yet.
    Dialling(Vec<Effect>),
}

/// Brings the live session to the document's connection.
///
/// `Ready` is what keeps the cycle in check: connecting moves the active document to
/// that connection's console, and activating a document moves the active connection to
/// the document's. Both sides have to be no-ops once satisfied.
fn switch_to_document_connection(model: &mut Model, index: usize) -> Switch {
    let Some(id) = model
        .documents
        .get(index)
        .and_then(|document| document.connection_id.clone())
        .or_else(|| {
            // Unbound means "wherever you are", which is what it would have run on
            // anyway. Recording it is what lets the tab say so from then on.
            let active = active_connection_uuid(model)?;
            model.documents.get_mut(index)?.connection_id = Some(active.clone());
            Some(active)
        })
    else {
        return Switch::Ready;
    };
    let Some(profile) = profile_by_uuid(model, &id) else {
        return Switch::Ready;
    };
    if model.connection.name == profile.name && model.active_session.is_some() {
        return Switch::Ready;
    }
    match model.connections.session_for(&profile.name).cloned() {
        Some(session) => Switch::Activated(activate_existing_session(model, &profile, session)),
        None => Switch::Dialling(connect_to(model, profile)),
    }
}

/// Runs an execution on the document's own connection. A document restored from a
/// launch, or one whose connection dropped, would otherwise send its query to whatever
/// session happens to be live -- the wrong database, silently.
fn execute_on_document_connection(model: &mut Model, action: Action) -> Vec<Effect> {
    let mut effects = match switch_to_document_connection(model, model.active_document) {
        Switch::Ready => Vec::new(),
        Switch::Activated(effects) => effects,
        Switch::Dialling(effects) => {
            // Replay when `ConnectionChanged` says the dial landed. Running now would
            // send the query to the session that happens to still be live.
            model.pending_execute = Some(crate::model::PendingExecute {
                document: model.active_document().id.clone(),
                action,
                token: model.connect_token,
            });
            return effects;
        }
    };
    match action {
        Action::ExecuteStatement => {
            if model.active_document().selection().is_some() {
                crate::screens::workbench::execute_selection(model);
            } else {
                crate::screens::workbench::execute_current_statement(model);
            }
        }
        Action::ExecuteSelection => crate::screens::workbench::execute_selection(model),
        Action::ExecuteDocument => crate::screens::workbench::execute_document(model),
        Action::RefreshTableData => {
            effects.extend(refresh_table_data(model));
            return effects;
        }
        _ => return effects,
    }
    effects.extend(start_query(model));
    effects
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
    let has_stored_documents = !state.documents.is_empty();
    if has_stored_documents {
        model.documents = state
            .documents
            .into_iter()
            .map(document_from_stored)
            .collect();
        model.active_document = 0;
    } else {
        // A first launch, or a project whose tabs were all closed: nothing is open, and
        // the workbench says so rather than inventing a `scratch.sql` for no connection.
        model.documents = vec![crate::model::EditorDocument::placeholder()];
        model.active_document = 0;
    }
    let recovery = state.recovery;
    // Recovery documents are autosaves. Restore them even if the previous
    // process managed to mark itself clean before its final document flush.
    let should_recover = recovery.needs_recovery() || !recovery.documents.is_empty();
    if should_recover && !recovery.documents.is_empty() {
        if !has_stored_documents {
            model.documents.clear();
        }
        restore_recovery_documents(model, recovery.documents);
    }
    let layout = if should_recover {
        recovery.layout.or(state.layout)
    } else {
        state.layout
    };
    apply_layout(model, layout);
    model.connections.load_profiles(state.connections);
    rename_restored_consoles(model);
    sync_explorer_connections(model);
    model.editor.snippets = state.snippets;
    model.recent_sql_files = state.recent_sql_files;
    apply_saved_settings(model);
}

/// A console stored before consoles were named after their connection comes back as
/// `console.sql`, which is what made a row of them unreadable. Done once the profiles
/// are loaded, since that is where the name comes from.
fn rename_restored_consoles(model: &mut Model) {
    let names: Vec<(String, String)> = model
        .connections
        .profiles
        .iter()
        .map(|row| (row.profile.id.0.to_string(), row.profile.name.clone()))
        .collect();
    for document in &mut model.documents {
        if document.title != "console.sql" {
            continue;
        }
        let Some(id) = document.connection_id.as_deref() else {
            continue;
        };
        if let Some((_, name)) = names.iter().find(|(profile, _)| profile == id) {
            document.title = name.clone();
        }
    }
}

pub fn rename_restored_consoles_for_test(model: &mut Model) {
    rename_restored_consoles(model);
}

fn restore_recovery_documents(model: &mut Model, documents: Vec<dexo_storage::RecoveryDocument>) {
    for checkpoint in documents {
        let mut recovered = crate::model::EditorDocument::with_text(&checkpoint.content);
        recovered.id = checkpoint.id;
        recovered.title = checkpoint.title;
        if let Some(existing) = model
            .documents
            .iter_mut()
            .find(|document| document.id == recovered.id)
        {
            recovered.path = existing.path.clone();
            *existing = recovered;
        } else {
            model.documents.push(recovered);
        }
    }
    model.active_document = model
        .active_document
        .min(model.documents.len().saturating_sub(1));
}
/// A connection's console lives at `sql/<connection uuid>/console.sql`, so a document
/// stored before the binding column existed still says which connection it belongs to.
/// Reading it back beats leaving every console from before the migration unlabelled.
fn connection_id_from_console_path(path: Option<&std::path::Path>) -> Option<String> {
    let parent = path?.parent()?.file_name()?.to_str()?;
    uuid::Uuid::parse_str(parent).ok().map(|id| id.to_string())
}

/// Restores one stored row. Public for the tests that pin the binding recovered from a
/// console's path, which is the only thing standing between a pre-migration workspace
/// and a strip of unlabelled tabs.
pub fn document_from_stored_for_test(
    stored: dexo_storage::StoredDocument,
) -> crate::model::EditorDocument {
    document_from_stored(stored)
}

fn document_from_stored(stored: dexo_storage::StoredDocument) -> crate::model::EditorDocument {
    let mut document = crate::model::EditorDocument::with_text(&stored.content);
    document.id = stored.id;
    document.title = stored.title;
    document.path = stored.path.map(std::path::PathBuf::from);
    document.connection_id = stored
        .connection_id
        .or_else(|| connection_id_from_console_path(document.path.as_deref()));
    if let Some(kind) = stored
        .kind
        .as_deref()
        .and_then(crate::model::DocumentKind::from_storage_tag)
    {
        document.kind = kind;
    }
    document
}

fn apply_layout(model: &mut Model, layout: Option<dexo_storage::WorkbenchLayout>) {
    let Some(layout) = layout.map(|layout| layout.clamp(model.width, model.height)) else {
        return;
    };
    model.panes.explorer_visible = layout.explorer_visible;
    model.panes.results_visible = layout.results_visible;
    model.panes.explorer_width = layout.explorer_width;
    model.panes.results_height = layout.results_height;
    model.panes.console_height = layout.console_height;
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
        model.results.push_tab(tab);
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

pub(crate) fn catalog_database(model: &Model) -> String {
    if model.schema.is_empty() {
        model.connection.name.clone()
    } else {
        model.schema.clone()
    }
}

fn catalog_followup_effects(model: &Model, capture: bool) -> Vec<Effect> {
    let mut effects = Vec::new();
    if capture && let Some(session) = model.active_session {
        // The previous capture answers completion while the new one walks the database.
        effects.push(Effect::LoadCompletionCatalog {
            connection_id: model.connection.name.clone(),
            database_name: catalog_database(model),
            generation: model.session_generation,
        });
        effects.push(Effect::CaptureCatalogSnapshot {
            connection_id: model.connection.name.clone(),
            database_name: catalog_database(model),
            session,
            generation: model.session_generation,
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
    // Enter only walks the tree; the table's rows open from the actions menu or `o`.
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
    if !model
        .explorer
        .selected_node()
        .is_some_and(|node| crate::screens::explorer::opens_table_data(&node.kind))
    {
        model
            .messages
            .warn("Select a table or view to open its data.".into());
        return Vec::new();
    }
    let mut effects = open_object_data(model);
    effects.extend(load_inspector(model));
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
    }
    effects
}

fn refresh_catalog(model: &mut Model, all: bool) -> Vec<Effect> {
    if model.active_session.is_none() {
        model
            .messages
            .warn("connect a session to refresh the catalog".into());
        return Vec::new();
    }
    let operation = crate::runtime::OperationId::new();
    if all {
        if model.connection.name.is_empty() {
            return Vec::new();
        }
        let connection = crate::screens::explorer::connection_id(&model.connection.name);
        model.explorer.expand_with(&connection, operation);
        return catalog_load_effect(model, Some(connection), operation, false);
    }
    // Refreshing a folder sends `folder:...` to the driver, which cannot parse it
    // and answers with an empty list -- emptying the folder for good.
    if model
        .explorer
        .selected_node()
        .is_some_and(crate::screens::explorer::is_folder_node)
    {
        return Vec::new();
    }
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    model.explorer.expand_with(&id, operation);
    catalog_load_effect(model, Some(id), operation, false)
}

fn discard_all_pending(model: &mut Model) {
    let mut inserted_rows: Vec<usize> = model
        .data
        .row_changes
        .iter()
        .filter(|(_, state)| matches!(state, dexo_app::data::RowEditState::Inserted))
        .map(|(&index, _)| index)
        .collect();
    inserted_rows.sort_unstable_by(|a, b| b.cmp(a));
    for index in inserted_rows {
        model.results.remove_row(index);
    }
    model.data.row_changes.clear();
    model.data.changes.discard();
}

fn submit_insert_row(model: &mut Model) -> Vec<Effect> {
    let values = model.data.insert_form.values();
    model.data.insert_form.close();
    if model.data.table.columns.is_empty() {
        return Vec::new();
    }
    model.data.changes.insert(values.clone());
    if !model.data.changes.errors().is_empty() {
        for error in model.data.changes.errors().to_vec() {
            model.messages.error(error);
        }
        return Vec::new();
    }
    let row_index = model.results.row_count();
    let row_values: Vec<DbValue> = model
        .results
        .columns()
        .iter()
        .map(|column| {
            values
                .iter()
                .find(|(name, _)| name == &column.name)
                .map(|(_, value)| value.clone())
                .unwrap_or(DbValue::Null)
        })
        .collect();
    model.results.append_rows(vec![row_values]);
    model
        .data
        .row_changes
        .insert(row_index, dexo_app::data::RowEditState::Inserted);
    Vec::new()
}

fn toggle_row_delete(model: &mut Model) -> Vec<Effect> {
    let Some(row_index) = model.results.cursor_row() else {
        return Vec::new();
    };
    match model.data.row_changes.get(&row_index).copied() {
        Some(dexo_app::data::RowEditState::Deleted) => {
            if let Some(identity) = row_identity_at(model, row_index)
                && let Some(position) = find_pending_index(
                    &model.data.changes,
                    |change| matches!(change, dexo_app::data::PendingChange::Delete { identity: existing, .. } if existing == &identity),
                )
            {
                model.data.changes.revert(position);
            }
            model.data.row_changes.remove(&row_index);
        }
        Some(dexo_app::data::RowEditState::Inserted) => {
            let Some(original) = row_original_at(model, row_index) else {
                return Vec::new();
            };
            if let Some(position) = find_pending_index(
                &model.data.changes,
                |change| matches!(change, dexo_app::data::PendingChange::Insert { values } if is_subset_of(values, &original)),
            ) {
                model.data.changes.revert(position);
            }
            model.results.remove_row(row_index);
            model.data.row_changes.remove(&row_index);
            let shifted: std::collections::BTreeMap<usize, dexo_app::data::RowEditState> = model
                .data
                .row_changes
                .iter()
                .map(|(&index, &state)| {
                    if index > row_index {
                        (index - 1, state)
                    } else {
                        (index, state)
                    }
                })
                .collect();
            model.data.row_changes = shifted;
        }
        _ => {
            let Some(identity) = row_identity_at(model, row_index) else {
                model.messages.warn(
                    "this table has no primary key or unique column, so rows cannot be deleted"
                        .into(),
                );
                return Vec::new();
            };
            let Some(original) = row_original_at(model, row_index) else {
                return Vec::new();
            };
            model.data.changes.delete(identity, original);
            model
                .data
                .row_changes
                .insert(row_index, dexo_app::data::RowEditState::Deleted);
        }
    }
    Vec::new()
}

fn row_identity_at(model: &Model, row_index: usize) -> Option<dexo_app::data::RowIdentity> {
    let identity_cols = dexo_app::data::RowIdentity::from_table(&model.data.table)?;
    let row = model.results.rows().get(row_index)?;
    let columns = model.results.columns();
    let mut values = Vec::with_capacity(identity_cols.len());
    for name in &identity_cols {
        let position = columns.iter().position(|column| &column.name == name)?;
        values.push(row.get(position)?.clone());
    }
    Some(dexo_app::data::RowIdentity {
        columns: identity_cols,
        values,
    })
}

fn row_original_at(
    model: &Model,
    row_index: usize,
) -> Option<Vec<(String, dexo_driver_api::DbValue)>> {
    let row = model.results.rows().get(row_index)?;
    let columns = model.results.columns();
    Some(
        columns
            .iter()
            .zip(row.iter())
            .map(|(column, value)| (column.name.clone(), value.clone()))
            .collect(),
    )
}

fn find_pending_index(
    changes: &dexo_app::data::ChangeSet,
    predicate: impl Fn(&dexo_app::data::PendingChange) -> bool,
) -> Option<usize> {
    changes.pending().iter().position(predicate)
}

/// Whether every `(name, value)` pair in `values` also appears in `row` — order-
/// independent, and correct even though `values` only holds the fields the user
/// actually typed (empty fields are omitted, not sent as `Null`), while `row` is
/// always the full column snapshot.
fn is_subset_of(values: &[(String, DbValue)], row: &[(String, DbValue)]) -> bool {
    values.iter().all(|(name, value)| {
        row.iter()
            .any(|(other_name, other_value)| other_name == name && other_value == value)
    })
}

fn activate_document(model: &mut Model, index: usize) -> Vec<Effect> {
    if index >= model.documents.len() {
        return Vec::new();
    }
    model.active_document = index;
    let mut effects = match switch_to_document_connection(model, index) {
        Switch::Ready => Vec::new(),
        Switch::Activated(effects) | Switch::Dialling(effects) => effects,
    };
    if model.documents[index].kind.is_table() {
        effects.extend(load_table_document(model, index));
    }
    effects
}

fn document_index_for_table(
    model: &Model,
    target: &dexo_driver_api::QualifiedName,
) -> Option<usize> {
    model.documents.iter().position(|document| {
        matches!(&document.kind, crate::model::DocumentKind::Table(existing) if existing == target)
    })
}

fn open_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    if model.active_session.is_none() {
        model
            .messages
            .warn("connect a session to browse table data".into());
        return Vec::new();
    }
    let target = dexo_app::parse_qualified(&node.qualified);
    let index = match document_index_for_table(model, &target) {
        Some(index) => index,
        None => {
            let connection_id = active_connection_uuid(model);
            model
                .documents
                .push(crate::model::EditorDocument::new_table(
                    target,
                    connection_id,
                ));
            model.documents.len() - 1
        }
    };
    model.set_active_document(index);
    model.data.last_error = None;
    load_table_document(model, index)
}

fn load_table_document(model: &mut Model, index: usize) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    if reload_would_orphan_edits(model) {
        return Vec::new();
    }
    let crate::model::DocumentKind::Table(target) = model.documents[index].kind.clone() else {
        return Vec::new();
    };
    model.data.target = target.clone();
    model.data.loading = true;
    model.data.page_offset = 0;
    model.data.target_document = Some(model.documents[index].id.clone());
    model.data.request_started = Some(std::time::Instant::now());
    if model.documents[index].console_log.is_empty() {
        model.documents[index]
            .console_log
            .push(format!("[{}] Connected", format_clock()));
    }
    match crate::runtime::data_manager::table_request(
        target.clone(),
        Vec::new(),
        model.data.filter.clone(),
        model.data.sort.clone(),
        model.data.page_offset,
        model.data.page_limit,
    ) {
        Ok(request) => {
            model.documents[index].console_log.push(format!(
                "[{}] {}> SELECT * FROM {} LIMIT {}",
                format_clock(),
                target.display_unquoted(),
                target.display_unquoted(),
                model.data.page_limit
            ));
            vec![
                Effect::LoadTableData {
                    request,
                    session,
                    generation: model.session_generation,
                },
                Effect::LoadTableColumns {
                    target,
                    session,
                    generation: model.session_generation,
                },
            ]
        }
        Err(message) => {
            model.messages.error(message);
            Vec::new()
        }
    }
}

fn log_rows_retrieved(model: &mut Model, row_count: usize) {
    let Some(document_id) = model.data.target_document.clone() else {
        return;
    };
    let elapsed_ms = model
        .data
        .request_started
        .map(|started| started.elapsed().as_millis())
        .unwrap_or(0);
    let offset = model.data.page_offset;
    let Some(document) = model
        .documents
        .iter_mut()
        .find(|document| document.id == document_id)
    else {
        return;
    };
    document.console_log.push(format!(
        "[{}] {row_count} rows retrieved starting from {} in {elapsed_ms} ms",
        format_clock(),
        offset + 1
    ));
}

fn format_clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

fn change_data_page(model: &mut Model, offset: u64) -> Vec<Effect> {
    if model.active_session.is_none() {
        model.data.last_error = Some("connect a session first".into());
        return Vec::new();
    }
    if !model.active_document().kind.is_table() {
        model.data.last_error = Some("open a table first".into());
        return Vec::new();
    }
    if reload_would_orphan_edits(model) {
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

/// Reruns the active table's page with the filter, sort and offset it has.
fn refresh_table_data(model: &mut Model) -> Vec<Effect> {
    if !model.active_document().kind.is_table() {
        model
            .messages
            .warn("Refresh reloads a table's data; open a table from the sidebar.".into());
        return Vec::new();
    };
    if model.active_session.is_none() {
        model
            .messages
            .warn("connect a session to browse table data".into());
        return Vec::new();
    }
    change_data_page(model, model.data.page_offset)
}

/// Row edits are keyed by row index and a page load leaves them in place, so loading
/// rows under them would pin each edit to whatever row lands at its index.
fn reload_would_orphan_edits(model: &mut Model) -> bool {
    let pending = model.data.has_pending_edits();
    if pending {
        model
            .messages
            .warn("Apply or discard the pending changes before reloading the table.".into());
    }
    pending
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
            model.messages.error(error.to_string());
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
            model.messages.warn(format!("local-only: {reason}"));
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
    model.results.replace_tabs(tab);
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
            model.messages.error(message);
            Vec::new()
        }
    }
}

fn open_inspector_facet(
    model: &mut Model,
    facet: crate::screens::object_inspector::InspectorFacet,
) -> Vec<Effect> {
    let effects = load_inspector(model);
    model.inspector.open = true;
    model.inspector.facet = facet;
    model.inspector.scroll = 0;
    effects
}

/// Loads an object's metadata. It does not show anything -- see `open_inspector_facet`.
fn load_inspector(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    model.inspector = crate::screens::object_inspector::ObjectInspector::loading(&node.qualified);
    vec![Effect::LoadObjectInspector {
        id: node.id.clone(),
        session,
        generation: model.session_generation,
    }]
}

fn goto_definition(model: &mut Model) -> Vec<Effect> {
    let sql = model.active_document().text();
    let cursor = model.active_document().byte_cursor();
    let catalog = dexo_app::SnapshotCatalog::new(flatten_explorer(&model.explorer));
    let Some(target) = dexo_sql::definition_at(&sql, cursor, &catalog) else {
        model.messages.warn("no definition at cursor".into());
        return Vec::new();
    };
    let wanted = target.display_unquoted();
    if let Some(id) = find_qualified(&model.explorer, &wanted) {
        model.explorer.reveal(&id);
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
            // A folder's `qualified` is just its label ("Tables"), which would
            // shadow a real object that happens to share the name.
            if !crate::screens::explorer::is_folder_node(node)
                && (node.qualified == qualified
                    || qualified.starts_with(&format!("{}.", node.qualified))
                    || node.qualified.ends_with(&format!(".{qualified}"))
                    || qualified.ends_with(&format!(".{}", node.label)))
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
            model.messages.warn("DDL is not loaded".into());
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
                        .warn("connect a session to fetch the value".into());
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
        model.messages.warn("no related foreign key".into());
        return Vec::new();
    };
    let Some(filter) = related_filter(&fk, &model.data.related_row) else {
        model
            .messages
            .warn("foreign key is null; navigation disabled".into());
        return Vec::new();
    };
    let title = fk.referenced_table.display_unquoted();
    let origin = model.active_document().id.clone();
    // This used to push a title onto the workbench strip that `data_nav_back` never
    // popped, so walking foreign keys leaked a tab per hop. The referenced table gets
    // a document, reusing one if it is already open.
    let connection_id = active_connection_uuid(model);
    let index = document_index_for_table(model, &fk.referenced_table).unwrap_or_else(|| {
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                fk.referenced_table.clone(),
                connection_id,
            ));
        model.documents.len() - 1
    });
    // The referenced table keeps its own state, so the switch comes before any of it
    // is written, and the way back is recorded on that table.
    model.set_active_document(index);
    if reload_would_orphan_edits(model) {
        return Vec::new();
    }
    model.data.crumbs.push(origin);
    model.data.filter = Some(filter);
    model.data.related_open.push(title);
    load_table_document(model, index)
}

fn data_nav_back(model: &mut Model) -> Vec<Effect> {
    let Some(origin) = model.data.crumbs.pop() else {
        return Vec::new();
    };
    model.data.related_open.pop();
    // The document walked away from kept its rows and paging; back is a switch to it.
    // This used to load the origin table into the current document, under its table.
    match model
        .documents
        .iter()
        .position(|document| document.id == origin)
    {
        Some(index) => model.set_active_document(index),
        None => model
            .messages
            .warn("the document this came from is closed".into()),
    }
    Vec::new()
}

fn copy_grid(model: &mut Model, format: dexo_app::data::CopyFormat) -> Vec<Effect> {
    match model.results.copy(format, model.data.dialect) {
        Ok(text) if text.len() > 8 * 1024 * 1024 => {
            model
                .messages
                .warn("selection too large for clipboard; export to a file".into());
            Vec::new()
        }
        Ok(text) => vec![Effect::CopyToClipboard { text }],
        Err(message) => {
            model.messages.error(message);
            Vec::new()
        }
    }
}

fn apply_changes(model: &mut Model) -> Vec<Effect> {
    if model.connection.read_only {
        model.messages.warn("connection is read-only".into());
        return Vec::new();
    }
    if let Some(review) = &model.data.review
        && review.production
        && !review.confirmed
    {
        model
            .messages
            .warn("type the target to confirm production apply".into());
        return Vec::new();
    }
    let Some(session) = model.active_session else {
        model
            .messages
            .warn("connect a session to apply changes".into());
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
            model.messages.error(error.to_string());
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
        model.messages.info("ddl queued".into());
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

/// Text in "nothing open" means there is a document. It becomes one in place, named
/// the way Ctrl+N would name it and bound to the connection it is being written for --
/// never the unowned `scratch.sql` it used to be.
///
/// Keyed on the text, not on how it got there: typing, a paste, a snippet, a history
/// entry, SQL generated from a result all write the buffer, and a stand-in left holding
/// any of it would be dropped on the next flush.
fn promote_placeholder(model: &mut Model) {
    if !model.active_document().kind.is_placeholder() || model.active_document().sql.is_empty() {
        return;
    }
    let title = suggested_document_name(model);
    let connection_id = active_connection_uuid(model);
    let document = model.active_document_mut();
    document.kind = crate::model::DocumentKind::Console;
    document.title = title;
    document.connection_id = connection_id;
}

fn suggested_document_name(model: &Model) -> String {
    format!("query-{}.sql", model.documents.len())
}

fn open_new_document_prompt(model: &mut Model) {
    let default_name = suggested_document_name(model);
    model.document_name_prompt =
        crate::screens::document_name_prompt::DocumentNamePrompt::open_create(default_name);
}

fn open_rename_document_prompt(model: &mut Model) {
    if model.documents.is_empty() {
        return;
    }
    let index = model.active_document;
    let current = model.documents[index].title.clone();
    model.document_name_prompt =
        crate::screens::document_name_prompt::DocumentNamePrompt::open_rename(index, current);
}

fn submit_document_name_prompt(model: &mut Model) -> Vec<Effect> {
    use crate::screens::document_name_prompt::{DocumentNameIntent, normalize_document_name};

    let intent = model.document_name_prompt.intent;
    let fallback = model.document_name_prompt.default_name.clone();
    let name = match normalize_document_name(model.document_name_prompt.name.as_str(), &fallback) {
        Ok(name) => name,
        Err(error) => {
            model.document_name_prompt.error = Some(error);
            return Vec::new();
        }
    };

    model.document_name_prompt.open = false;
    model.document_name_prompt.error = None;

    match intent {
        Some(DocumentNameIntent::Create) => {
            let connection_id = active_connection_uuid(model);
            model
                .documents
                .push(crate::model::EditorDocument::new_unique(
                    name,
                    None,
                    connection_id,
                ));
            model.active_document = model.documents.len() - 1;
            model.focus_active_document_tab();
            model.focus = Focus::Editor;
        }
        Some(DocumentNameIntent::Rename) => {
            let index = model.document_name_prompt.document_index;
            if index < model.documents.len() {
                model.documents[index].title = name;
                model.sync_document_tabs_scroll();
            }
        }
        None => {}
    }
    Vec::new()
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
    if model.data.has_pending_edits() {
        model.data.query_prompt.error = Some("apply or discard the pending changes first".into());
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
    if model.active_document().kind.is_placeholder() {
        return Vec::new();
    }
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
    // Unsaved changes are the user's to keep or let go. Closing used to save on its own
    // or, with no file to save to, refuse -- there was no way to discard them.
    if model.active_document().is_dirty() {
        let document = model.active_document();
        model.close_prompt = Some(crate::model::ClosePrompt {
            document: document.id.clone(),
            title: document.title.clone(),
            choice: crate::model::CloseChoice::Save,
        });
        return Vec::new();
    }
    remove_document(model, model.active_document);
    Vec::new()
}

fn resolve_close(model: &mut Model, choice: crate::model::CloseChoice) -> Vec<Effect> {
    use crate::model::CloseChoice;
    let Some(prompt) = model.close_prompt.take() else {
        return Vec::new();
    };
    let Some(index) = model
        .documents
        .iter()
        .position(|document| document.id == prompt.document)
    else {
        return Vec::new();
    };
    match choice {
        CloseChoice::Cancel => Vec::new(),
        CloseChoice::Discard => {
            remove_document(model, index);
            vec![Effect::DiscardRecovery {
                document: prompt.document,
            }]
        }
        CloseChoice::Save => {
            model.active_document = index;
            // The buffer holds the only copy of these edits, so the tab survives until
            // `DocumentSaved` confirms the write. A failed save leaves the tab open with
            // the error in the message log; an untitled one goes through the picker, and
            // backing out of the picker keeps the tab.
            let document = model.active_document();
            let file_backed = document.path.is_some();
            model.pending_document_close = Some(crate::model::PendingDocumentClose {
                document: document.id.clone(),
                revision: document.sql.revision(),
            });
            if file_backed {
                model
                    .messages
                    .info("Saving dirty file before closing it.".into());
            }
            save_active_document(model)
        }
    }
}

fn handle_close_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::model::CloseChoice;
    let Some(prompt) = model.close_prompt.as_mut() else {
        return Vec::new();
    };
    match key.code {
        KeyCode::Esc => resolve_close(model, CloseChoice::Cancel),
        KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
            prompt.choice = prompt.choice.next();
            Vec::new()
        }
        KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
            prompt.choice = prompt.choice.prev();
            Vec::new()
        }
        KeyCode::Enter => {
            let choice = prompt.choice;
            resolve_close(model, choice)
        }
        KeyCode::Char('s') => resolve_close(model, CloseChoice::Save),
        KeyCode::Char('d') => resolve_close(model, CloseChoice::Discard),
        _ => Vec::new(),
    }
}

fn mouse_close_prompt(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    use crate::model::CloseChoice;
    match hit {
        Some(HitTarget::Button(HitButton::Confirm)) => resolve_close(model, CloseChoice::Save),
        Some(HitTarget::Button(HitButton::Discard)) => resolve_close(model, CloseChoice::Discard),
        Some(HitTarget::Button(HitButton::Cancel)) => resolve_close(model, CloseChoice::Cancel),
        _ => Vec::new(),
    }
}

fn remove_document(model: &mut Model, index: usize) {
    if index >= model.documents.len() {
        return;
    }
    model.documents.remove(index);
    if model.documents.is_empty() {
        model
            .documents
            .push(crate::model::EditorDocument::placeholder());
        model.active_document = 0;
    } else {
        if model.active_document > index {
            model.active_document -= 1;
        }
        model.active_document = model.active_document.min(model.documents.len() - 1);
    }
    model.focus_active_document_tab();
    model.focus = Focus::Editor;
}

fn cycle_mode(model: &mut Model, delta: i32) -> Vec<Effect> {
    let next = crate::theme::Mode::from_key(&model.settings.mode).step(delta);
    model.settings.mode = next.as_key().into();
    rebuild_theme(model);
    Vec::new()
}

fn cycle_accent(model: &mut Model, delta: i32) -> Vec<Effect> {
    model.settings.accent = crate::theme::step_accent(&model.settings.accent, delta).into();
    rebuild_theme(model);
    Vec::new()
}

/// The surface and the primary color are picked separately, then composed here.
fn rebuild_theme(model: &mut Model) {
    model.theme = crate::theme::theme_for(
        crate::theme::Mode::from_key(&model.settings.mode),
        &model.settings.accent,
    );
    persist_settings(model);
}

fn cycle_keymap(model: &mut Model, delta: i32) -> Vec<Effect> {
    let next = crate::keymap::step_profile(&model.keymap.name, delta);
    model.keymap = crate::keymap::Keymap::named(next);
    model.settings.keymap = model.keymap.name.clone();
    persist_settings(model);
    Vec::new()
}

/// `delta` is the arrow direction; the two-value rows ignore it because they toggle.
fn step_focused_setting(model: &mut Model, delta: i32) -> Vec<Effect> {
    match model.settings.focus {
        0 => cycle_mode(model, delta),
        1 => cycle_accent(model, delta),
        2 => cycle_keymap(model, delta),
        3 => update(model, Action::ToggleMouse),
        4 => update(model, Action::ToggleAnimation),
        5 => update(model, Action::ToggleUnicode),
        6 => update(model, Action::ToggleUpdateCheck),
        _ => update(model, Action::ConfirmResetSettings),
    }
}

/// Resetting has to reach the live state too, or the rows would report
/// defaults the app is not actually running with.
fn reset_settings_to_defaults(model: &mut Model) {
    model.mouse = true;
    model.animation = true;
    model.capabilities.unicode = true;
    model.theme = crate::theme::builtin_dark();
    model.keymap = crate::keymap::Keymap::default_profile();
    model.settings.reset();
    sync_settings_screen(model);
}

/// The settings rows mirror live state, which lives outside the screen.
fn sync_settings_screen(model: &mut Model) {
    model.settings.mouse = model.mouse;
    model.settings.animation = model.animation;
    model.settings.unicode = model.capabilities.unicode;
    model.settings.keymap = model.keymap.name.clone();
}

fn persist_settings(model: &Model) {
    let Ok(paths) = dexo_storage::AppPaths::discover() else {
        return;
    };
    let mut manager = crate::runtime::settings_manager::SettingsManager::load(&paths.data_dir);
    let next = dexo_app::settings::SettingsFile {
        mode: match crate::theme::Mode::from_key(&model.settings.mode) {
            crate::theme::Mode::LowColor => dexo_app::settings::ModeId::HighContrast,
            crate::theme::Mode::Light => dexo_app::settings::ModeId::Light,
            crate::theme::Mode::Dark => dexo_app::settings::ModeId::Dark,
        },
        accent: model.settings.accent.clone(),
        mouse: model.mouse,
        animation: model.animation,
        unicode: if model.capabilities.unicode {
            dexo_app::settings::UnicodeMode::Unicode
        } else {
            dexo_app::settings::UnicodeMode::Ascii
        },
        keymap: dexo_app::settings::KeymapConfig {
            run_statement: "Ctrl+Enter".into(),
            profile: model.keymap.name.clone(),
        },
        completion_trigger: model.settings.completion_trigger,
        update_check: model.settings.updates,
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
    model.animation = manager.active.animation;
    model.capabilities.unicode = matches!(
        manager.active.unicode,
        dexo_app::settings::UnicodeMode::Unicode
    );
    model.keymap = crate::keymap::Keymap::named(&manager.active.keymap.profile);
    let mode = crate::theme::mode_from_settings(manager.active.mode);
    model.settings.mode = mode.as_key().into();
    model.settings.accent = manager.active.accent.clone();
    model.settings.completion_trigger = manager.active.completion_trigger;
    model.settings.updates = manager.active.update_check;
    model.theme = crate::theme::theme_for(mode, &model.settings.accent);
    sync_settings_screen(model);
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

fn next_transfer_format(current: &str) -> String {
    match current {
        "csv" => "tsv",
        "tsv" => "json",
        "json" => "jsonl",
        "jsonl" => "sql",
        _ => "csv",
    }
    .into()
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
        KeyCode::Esc => cancel_file_picker(model),
        KeyCode::Tab => {
            model.file_picker.focus_next();
            Vec::new()
        }
        KeyCode::BackTab => {
            model.file_picker.focus_prev();
            Vec::new()
        }
        KeyCode::Down => {
            model.file_picker.move_down(rows);
            Vec::new()
        }
        KeyCode::Up => {
            model.file_picker.move_up(rows);
            Vec::new()
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Backspace
        | KeyCode::Delete
            if model.file_picker.focus == FilePickerFocus::Name =>
        {
            let _ = model.file_picker.name.handle_key(key);
            Vec::new()
        }
        KeyCode::Left
            if model.file_picker.focus == FilePickerFocus::List
                && model.file_picker.section
                    == crate::screens::file_picker::FilePickerSection::Browser =>
        {
            model.file_picker.parent();
            Vec::new()
        }
        KeyCode::Left => {
            model.file_picker.footer_left();
            Vec::new()
        }
        KeyCode::Right
            if model.file_picker.focus == FilePickerFocus::List
                && model.file_picker.section
                    == crate::screens::file_picker::FilePickerSection::Browser =>
        {
            let _ = model.file_picker.activate_selected();
            Vec::new()
        }
        KeyCode::Right => {
            model.file_picker.footer_right();
            Vec::new()
        }
        KeyCode::Backspace
            if model.file_picker.focus == FilePickerFocus::List
                && model.file_picker.section
                    == crate::screens::file_picker::FilePickerSection::Browser =>
        {
            model.file_picker.parent();
            Vec::new()
        }
        KeyCode::Char('h')
            if model.file_picker.focus == FilePickerFocus::List
                && model.file_picker.section
                    == crate::screens::file_picker::FilePickerSection::Browser =>
        {
            model.file_picker.toggle_hidden();
            Vec::new()
        }
        KeyCode::Char(ch)
            if model.file_picker.focus == FilePickerFocus::Name
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
        {
            let _ = model.file_picker.name.handle_key(key);
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
            cancel_file_picker(model)
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

/// Every way out of the picker without choosing. A close armed by "Save" in the
/// unsaved-changes prompt was waiting on this save; with no save coming, it is
/// disarmed, or a later Ctrl+S on the same document would close it by surprise.
fn cancel_file_picker(model: &mut Model) -> Vec<Effect> {
    model.file_picker.open = false;
    if model.file_picker_mode == crate::screens::file_picker::FilePickerMode::Save {
        model.pending_document_close = None;
    }
    Vec::new()
}

fn file_picker_submit(model: &mut Model) -> Vec<Effect> {
    let Some(path) = model.file_picker.chosen_path() else {
        model.file_picker.error = Some("choose a file or type a name".into());
        return Vec::new();
    };
    model.file_picker.open = false;
    match model.file_picker_mode {
        crate::screens::file_picker::FilePickerMode::Open => open_document_path(model, path),
        crate::screens::file_picker::FilePickerMode::Save => {
            let doc = model.active_document_mut();
            // Save As gives the document a new file, and the tab names the file it now
            // lives in. Only here: a plain save keeps a name the user chose with F2.
            if let Some(name) = path.file_name() {
                doc.title = name.to_string_lossy().into_owned();
            }
            doc.path = Some(path.clone());
            let effects = vec![Effect::SaveDocument(crate::action::DocumentIoRequest {
                document: doc.id.clone(),
                path,
                content: doc.text(),
                revision: doc.sql.revision(),
                expected_fingerprint: None,
            })];
            model.sync_document_tabs_scroll();
            effects
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
        crate::screens::file_picker::FilePickerMode::ConfigExport => {
            update(model, Action::ExportConfig { path })
        }
        crate::screens::file_picker::FilePickerMode::ConfigImport => {
            update(model, Action::ImportConfig { path })
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
            model.messages.error(message);
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
            .warn("type the project name to confirm".into());
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
    recent_sql_files: Vec<std::path::PathBuf>,
) {
    model.project = project.name.clone();
    model.project_id = project.id.0.to_string();
    model.projects.touch_recent(&project.name);
    model.projects.pending = None;
    model.recent_sql_files = recent_sql_files;
    if documents.is_empty() {
        model.documents = vec![crate::model::EditorDocument::placeholder()];
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
        | crate::screens::projects::ProjectsMode::Rename => {
            use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
            match footer_key(&mut model.projects.footer, &key) {
                FooterKey::Cancel => {
                    model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
                    model.projects.name_input.clear();
                    model.projects.error = None;
                    model.projects.footer = FooterFocus::Input;
                    return Vec::new();
                }
                FooterKey::Submit => return submit_project_name(model),
                FooterKey::Moved => return Vec::new(),
                FooterKey::Pass => {}
            }
            match key.code {
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
            }
        }
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
        KeyCode::Char('e') => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigExport,
            );
            Vec::new()
        }
        KeyCode::Char('i') => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigImport,
            );
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
    connect_selected(model)
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
        model.messages.warn("no query parameters".into());
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
        format!(
            "mode={} accent={} mouse={}",
            model.settings.mode, model.settings.accent, model.mouse
        ),
        String::new(),
    )
}

fn open_file_picker(model: &mut Model, mode: crate::screens::file_picker::FilePickerMode) {
    model.file_picker_mode = mode;
    let recents = if mode == crate::screens::file_picker::FilePickerMode::Open {
        model.recent_sql_files.as_slice()
    } else {
        &[]
    };
    model.file_picker.open_browser_with_recents(recents);
    if mode == crate::screens::file_picker::FilePickerMode::Save {
        let preset = model
            .active_document()
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        if let Some(name) = preset {
            model.file_picker.name.set_text(name);
        }
    }
}

fn normalize_document_path(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn document_index_for_path(model: &Model, path: &std::path::Path) -> Option<usize> {
    let normalized = normalize_document_path(path);
    model.documents.iter().position(|document| {
        document
            .path
            .as_ref()
            .map(|existing| normalize_document_path(existing) == normalized)
            .unwrap_or(false)
    })
}

fn touch_recent_sql_file(model: &mut Model, path: &std::path::Path) -> Vec<Effect> {
    let normalized = normalize_document_path(path);
    let path_str = normalized.to_string_lossy().into_owned();
    model
        .recent_sql_files
        .retain(|existing| existing.to_string_lossy() != path_str);
    model.recent_sql_files.insert(0, normalized.clone());
    model.recent_sql_files.truncate(20);
    if model.project_id.is_empty() {
        return Vec::new();
    }
    vec![Effect::TouchRecentSqlFile {
        project_id: model.project_id.clone(),
        path: path_str,
    }]
}

fn open_document_path(model: &mut Model, path: std::path::PathBuf) -> Vec<Effect> {
    if path.is_dir() {
        model.messages.warn("choose a file, not a directory".into());
        return Vec::new();
    }
    let normalized = normalize_document_path(&path);
    if let Some(index) = document_index_for_path(model, &normalized) {
        model.active_document = index;
        model.sync_document_tabs_scroll();
        return touch_recent_sql_file(model, &normalized);
    }
    let title = normalized
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled.sql".into());
    let connection_id = active_connection_uuid(model);
    let document =
        crate::model::EditorDocument::new_unique(title, Some(normalized.clone()), connection_id);
    // The load answers by this id; a fresh one here matched no tab, so the file's text
    // never arrived.
    let document_id = document.id.clone();
    model.documents.push(document);
    model.active_document = model.documents.len().saturating_sub(1);
    model.sync_document_tabs_scroll();
    let mut effects = touch_recent_sql_file(model, &normalized);
    effects.push(Effect::LoadDocument(crate::action::DocumentIoRequest {
        document: document_id,
        path: normalized,
        content: String::new(),
        revision: 0,
        expected_fingerprint: None,
    }));
    effects
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
        PaletteInvocation::OpenFlow(FlowIntent::ConnectionDelete) => {
            // Deleting takes a confirmation, and the connections screen is the only
            // place that draws one -- the flow is that screen, opened on the target.
            let effects = update(model, Action::DeleteConnection);
            if model.connections.delete_target.is_some() {
                model.connections.open = true;
            }
            effects
        }
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
        model.messages.warn(reason.clone());
        return Vec::new();
    }
    let invocation = entry.invocation.clone();
    close_palette(model);
    invoke_palette(model, invocation)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use dexo_driver_api::DbValue;

    use super::update;
    use crate::action::{Action, Effect};
    use crate::model::{Focus, Model};
    use crate::runtime::{OperationId, OperationKey};

    #[test]
    fn alt_e_hides_and_reshows_the_explorer_panel() {
        let mut model = Model::default();
        assert!(model.panes.explorer_visible);

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)),
        );
        assert!(!model.panes.explorer_visible);

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)),
        );
        assert!(model.panes.explorer_visible);
    }

    #[test]
    fn hiding_the_focused_explorer_panel_moves_focus_to_the_editor() {
        let mut model = Model {
            focus: Focus::Explorer,
            ..Model::default()
        };

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)),
        );

        assert!(!model.panes.explorer_visible);
        assert_eq!(model.focus, Focus::Editor);
    }

    #[test]
    fn alt_r_hides_and_reshows_the_results_panel() {
        let mut model = Model::default();
        assert!(model.panes.results_visible);

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT)),
        );
        assert!(!model.panes.results_visible);

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT)),
        );
        assert!(model.panes.results_visible);
    }

    #[test]
    fn refocusing_a_hidden_results_panel_shows_it_again() {
        let mut model = Model::default();

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT)),
        );
        assert!(!model.panes.results_visible);

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT)),
        );
        assert!(model.panes.results_visible);
        assert_eq!(model.focus, Focus::Results);
    }

    #[test]
    fn typing_while_help_is_open_appends_to_search_query_and_resets_scroll() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.scroll = 5;

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
        );

        assert_eq!(model.help.query, "d");
        assert_eq!(model.help.scroll, 0);
    }

    #[test]
    fn backspace_while_help_is_open_removes_last_search_char() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = "disc".into();

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        );

        assert_eq!(model.help.query, "dis");
    }

    #[test]
    fn question_mark_is_typed_into_the_help_search_instead_of_closing_it() {
        let mut model = Model::default();
        model.help.open = true;

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
        );

        assert!(model.help.open);
        assert_eq!(model.help.query, "?");
    }

    #[test]
    fn esc_still_closes_help_and_clears_its_search_query() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = "disc".into();

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );

        assert!(!model.help.open);
        assert_eq!(model.help.query, "");
    }

    #[test]
    fn reopening_help_resets_a_previous_search_query() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = "disc".into();

        update(&mut model, Action::ToggleHelp);
        assert!(!model.help.open);
        update(&mut model, Action::ToggleHelp);

        assert!(model.help.open);
        assert_eq!(model.help.query, "");
    }

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
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

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

    #[test]
    fn open_document_path_creates_tab_and_deduplicates_by_path() {
        let dir = tempfile::tempdir().unwrap();
        // Paths are stored resolved, and macOS's temp dir sits behind the /var ->
        // /private/var symlink.
        let path = std::fs::canonicalize(dir.path()).unwrap().join("query.sql");
        std::fs::write(&path, "select 1").unwrap();
        let mut model = Model {
            project_id: "project-1".into(),
            ..Model::default()
        };
        let first = super::open_document_path(&mut model, path.clone());
        assert!(
            first
                .iter()
                .any(|effect| matches!(effect, Effect::LoadDocument { .. }))
        );
        assert_eq!(model.documents.len(), 2);
        assert_eq!(model.active_document, 1);
        assert_eq!(model.documents[1].path.as_ref(), Some(&path));
        assert_eq!(
            model.recent_sql_files.first().map(|p| p.as_path()),
            Some(path.as_path())
        );

        let second = super::open_document_path(&mut model, path.clone());
        assert!(
            second
                .iter()
                .all(|effect| !matches!(effect, Effect::LoadDocument { .. }))
        );
        assert_eq!(model.documents.len(), 2);
        assert_eq!(model.active_document, 1);
    }

    /// The picker's load carried an id no tab had, so the file's text never arrived and
    /// Ctrl+O opened an empty tab. The load also rebuilt the tab from scratch, dropping
    /// the connection it belongs to.
    #[test]
    fn a_file_opened_from_the_picker_shows_its_text_and_keeps_its_connection() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join("query.sql");
        std::fs::write(&path, "select 42").unwrap();
        let mut model = Model::default();

        let effects = super::open_document_path(&mut model, path.clone());
        let request = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::LoadDocument(request) => Some(request.clone()),
                _ => None,
            })
            .expect("opening did not load the file");
        model.active_document_mut().connection_id = Some("conn-1".into());
        update(
            &mut model,
            Action::DocumentLoaded {
                document: request.document,
                path: request.path,
                content: "select 42".into(),
            },
        );

        let document = model.active_document();
        assert_eq!(document.text(), "select 42", "the tab opened empty");
        assert!(!document.is_dirty());
        assert_eq!(document.path.as_ref(), Some(&path));
        assert_eq!(document.connection_id.as_deref(), Some("conn-1"));
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

    /// The document strip is always on screen now, so switching files is a workspace
    /// action, not an editor one. Alt+Left/Right lived in the `[editor]` context and the
    /// action itself bailed unless the focus was the editor -- a table document, whose
    /// grid takes the editor's pane, has neither.
    #[test]
    fn alt_arrows_switch_documents_from_the_results_pane() {
        use crate::action::FocusTarget;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut model = Model::default();
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                dexo_app::parse_qualified("public.orders"),
                None,
            ));
        model.active_document = 1;
        model.focus_active_document_tab();
        update(&mut model, Action::Focus(FocusTarget::Editor));
        assert_eq!(model.effective_focus(), Focus::Results);

        let alt = |code| Action::Key(KeyEvent::new(code, KeyModifiers::ALT));
        update(&mut model, alt(KeyCode::Left));
        assert_eq!(model.active_document, 0, "alt+left switched nothing");
        update(&mut model, alt(KeyCode::Right));
        assert_eq!(model.active_document, 1, "alt+right switched nothing");
    }

    fn table_document_on_page_two() -> Model {
        let orders = dexo_app::parse_qualified("public.orders");
        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                orders.clone(),
                None,
            ));
        model.active_document = model.documents.len() - 1;
        model.data.target = orders;
        model.data.target_document = Some(model.active_document().id.clone());
        model.data.page_offset = u64::from(model.data.page_limit);
        model.focus = Focus::Results;
        model
    }

    fn ctrl_r() -> Action {
        Action::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
    }

    fn table_requests(effects: &[Effect]) -> Vec<&dexo_driver_api::DataRequest> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::LoadTableData { request, .. } => Some(request),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ctrl_r_reloads_the_table_on_the_page_it_is_on() {
        let mut model = table_document_on_page_two();
        let offset = model.data.page_offset;

        let effects = update(&mut model, ctrl_r());

        let requests = table_requests(&effects);
        assert_eq!(requests.len(), 1, "{effects:?}");
        assert_eq!(
            requests[0].object,
            dexo_app::parse_qualified("public.orders")
        );
        assert_eq!(requests[0].page.offset, offset);
        assert!(model.data.loading);
    }

    #[test]
    fn refresh_waits_for_pending_edits_to_be_applied_or_discarded() {
        let mut model = table_document_on_page_two();
        model
            .data
            .row_changes
            .insert(0, dexo_app::data::RowEditState::Inserted);

        let effects = update(&mut model, ctrl_r());

        assert!(table_requests(&effects).is_empty(), "{effects:?}");
        assert_eq!(
            model.messages.last().map(|entry| entry.message.as_str()),
            Some("Apply or discard the pending changes before reloading the table.")
        );
    }

    /// The data state is shared, so after another table loaded it still names that
    /// table; refreshing must load the table on screen, not the one loaded last.
    #[test]
    fn refresh_loads_the_table_on_screen_not_the_last_one_loaded() {
        let mut model = table_document_on_page_two();
        model.data.target = dexo_app::parse_qualified("public.customers");

        let effects = update(&mut model, ctrl_r());

        let requests = table_requests(&effects);
        assert_eq!(requests.len(), 1, "{effects:?}");
        assert_eq!(
            requests[0].object,
            dexo_app::parse_qualified("public.orders")
        );
        assert_eq!(requests[0].page.offset, 0);
    }

    #[test]
    fn refresh_outside_a_table_document_loads_nothing() {
        let mut model = table_document_on_page_two();
        model.active_document = 0;

        let effects = update(&mut model, Action::RefreshTableData);

        assert!(table_requests(&effects).is_empty(), "{effects:?}");
    }

    /// Rows, cells and headers focus the grid themselves; the rest of its pane went
    /// through the positional pane-3 focus, which in a table document is the console.
    #[test]
    fn clicking_a_table_documents_panes_focuses_each() {
        let mut model = Model::default();
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                dexo_app::parse_qualified("public.orders"),
                None,
            ));
        model.active_document = model.documents.len() - 1;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| crate::render::render(frame, &model, &mut hits))
            .unwrap();
        let (column, row) = hits.center(crate::mouse::HitTarget::Grid);
        model.hits = hits;
        assert_eq!(
            model.hits.at(column, row),
            Some(crate::mouse::HitTarget::Grid)
        );

        update(
            &mut model,
            Action::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }),
        );

        assert_eq!(model.effective_focus(), Focus::Results);

        let (column, row) = model.hits.center(crate::mouse::HitTarget::Console);
        update(
            &mut model,
            Action::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }),
        );

        assert_eq!(model.effective_focus(), Focus::Console);
    }

    /// A table document puts the grid in pane 2 and the console in pane 3. Alt+2 and
    /// Alt+3 are positional, so they used to land one pane off: Alt+2 focused an editor
    /// that is not on screen, and Alt+3 focused the grid.
    #[test]
    fn pane_focus_follows_what_a_table_document_actually_shows() {
        let mut model = Model::default();
        model
            .documents
            .push(crate::model::EditorDocument::new_table(
                dexo_app::parse_qualified("public.orders"),
                None,
            ));
        model.active_document = model.documents.len() - 1;

        use crate::action::FocusTarget;

        update(&mut model, Action::Focus(FocusTarget::Editor));
        let view = crate::render::render_to_string(&model, 160, 50);
        assert!(
            view.contains("▸ Results"),
            "pane 2 is the grid here:\n{view}"
        );

        update(&mut model, Action::Focus(FocusTarget::Results));
        let view = crate::render::render_to_string(&model, 160, 50);
        assert!(
            view.contains("▸ Console"),
            "pane 3 is the console here:\n{view}"
        );
        assert!(
            !view.contains("▸ Results"),
            "the grid kept the highlight:\n{view}"
        );

        // The same slip happens without touching Alt at all: sit in the editor, open a
        // table from the tree, and the focus left behind points at no pane on screen.
        model.focus = Focus::Editor;
        assert!(
            crate::render::render_to_string(&model, 160, 50).contains("▸ Results"),
            "a stale editor focus left nothing highlighted"
        );
        model.active_document = 0;
        model.focus = Focus::Console;
        let view = crate::render::render_to_string(&model, 160, 50);
        assert!(
            view.contains("▸ Results"),
            "console focus outlived the console:\n{view}"
        );
    }

    /// The output pane used to have two cyclers: one for Grid/Explain and one for the
    /// Explain sub-view. One flat ring is one question instead of two nested ones.
    #[test]
    fn output_view_cycles_through_every_projection_once() {
        use crate::model::ResultsView;
        use crate::screens::explain::ExplainView;

        let mut model = Model::default();
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push((model.results.view, model.explain.view));
            update(&mut model, Action::CycleResultsView);
        }

        assert_eq!(
            seen,
            [
                (ResultsView::Grid, ExplainView::Tree),
                (ResultsView::Explain, ExplainView::Tree),
                (ResultsView::Explain, ExplainView::Table),
                (ResultsView::Explain, ExplainView::Summary),
                (ResultsView::Messages, ExplainView::Summary),
            ]
        );
        assert_eq!(model.results.view, ResultsView::Grid, "the ring must close");
    }

    /// A plan used to arrive and force `tabs.active = 4`, yanking the user out of the
    /// editor mid-keystroke. It belongs in the output pane, which nobody is looking at
    /// while they type.
    /// DDL and Properties describe the tree selection, not the open document, so they
    /// must not disturb it -- and they have to close.
    #[test]
    fn object_metadata_opens_as_an_overlay_and_closes() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        for (action, facet) in [
            (
                Action::OpenObjectDdl,
                crate::screens::object_inspector::InspectorFacet::Ddl,
            ),
            (
                Action::OpenObjectInspector,
                crate::screens::object_inspector::InspectorFacet::Properties,
            ),
        ] {
            let mut model = Model::default();
            model.set_sql("select 1");
            model.active_session = Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1)));
            model.explorer.replace_roots(dexo_driver_api::CatalogList {
                objects: vec![dexo_driver_api::CatalogObject::new(
                    dexo_driver_api::ObjectId::new("table:orders"),
                    dexo_driver_api::ObjectKind::Table,
                    dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "orders"),
                    None,
                )],
                restrictions: vec![],
            });
            model
                .explorer
                .select(dexo_driver_api::ObjectId::new("table:orders"));
            let document = model.active_document().id.clone();

            update(&mut model, action);
            assert!(model.inspector.open, "{facet:?} did not open");
            assert_eq!(model.inspector.facet, facet);
            assert_eq!(model.active_document().id, document);

            update(
                &mut model,
                Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            );
            assert!(!model.inspector.open, "{facet:?} would not close");
        }
    }

    #[test]
    fn explain_result_arrives_without_stealing_the_editor() {
        let mut model = Model::default();
        model.set_sql("select 1");
        model.focus = Focus::Editor;
        let document = model.active_document().id.clone();

        let plan = crate::screens::explain::ExplainScreen::fixture()
            .plan
            .expect("fixture plan");
        update(
            &mut model,
            Action::ExplainLoaded {
                plan: Box::new(plan),
            },
        );

        assert_eq!(model.focus, Focus::Editor, "explain moved the focus");
        assert_eq!(model.active_document().id, document);
        assert_eq!(model.results.view, crate::model::ResultsView::Explain);
    }

    /// A restored table browser has to still be a table browser. When the kind was left
    /// out of the row, it came back as an ordinary editor tab, `document_index_for_table`
    /// no longer recognised it, and the next open of that table pushed a second document
    /// -- once per table per launch, which is how the tab strip fills with thousands of
    /// duplicates of the same handful of tables.
    #[test]
    fn reopening_a_restored_table_finds_its_tab_instead_of_making_another() {
        use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind};

        let target = dexo_app::parse_qualified("public.brands");
        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        model
            .documents
            .push(super::document_from_stored(dexo_storage::StoredDocument {
                id: "doc-brands".into(),
                project_id: Some("p1".into()),
                title: "brands".into(),
                content: "SELECT * FROM public.brands LIMIT 501".into(),
                path: None,
                fingerprint: None,
                kind: crate::model::DocumentKind::Table(target.clone()).storage_tag(),
                connection_id: None,
            }));
        assert!(
            model.documents[1].kind.is_table(),
            "the kind did not survive the row"
        );

        model.explorer.replace_roots(CatalogList {
            objects: vec![CatalogObject::new(
                ObjectId::new("table:brands"),
                ObjectKind::Table,
                target,
                None,
            )],
            restrictions: vec![],
        });
        model.explorer.select(ObjectId::new("table:brands"));
        update(&mut model, Action::OpenObjectData);

        assert_eq!(
            model.documents.len(),
            2,
            "opening the table made a second tab"
        );
        assert_eq!(model.active_document().id, "doc-brands");
    }

    /// The strip lights the tab under its cursor, and opening a table from the tree
    /// moved the active document without moving the cursor: the strip said one file
    /// was open while another was.
    #[test]
    fn opening_a_table_from_the_tree_moves_the_tab_cursor_with_it() {
        use dexo_driver_api::{CatalogList, ObjectId, ObjectKind};

        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        model
            .documents
            .push(crate::model::EditorDocument::new_unique(
                "q2.sql", None, None,
            ));
        update(&mut model, Action::SelectDocument { index: 1 });
        assert_eq!(
            model.document_tab_focus,
            crate::model::DocumentTabFocus::Document(1)
        );
        model.explorer.replace_roots(CatalogList {
            objects: vec![catalog_object("table:orders", ObjectKind::Table, "orders")],
            restrictions: vec![],
        });
        model.explorer.select(ObjectId::new("table:orders"));

        update(&mut model, Action::OpenObjectData);

        assert!(model.active_document().kind.is_table());
        assert_eq!(
            model.document_tab_focus,
            crate::model::DocumentTabFocus::Document(model.active_document),
            "the strip lights a tab that is not the open one"
        );
    }

    /// Enter walks the tree: on a table it shows the columns and indexes and leaves the
    /// rows alone. Opening the data is the actions menu's first entry, or `o`.
    #[test]
    fn enter_on_a_table_expands_it_and_o_opens_its_data() {
        use dexo_driver_api::{CatalogList, ObjectId, ObjectKind};

        let mut model = Model {
            session_generation: 1,
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            focus: Focus::Explorer,
            ..Model::default()
        };
        model.explorer.replace_roots(CatalogList {
            objects: vec![catalog_object("table:orders", ObjectKind::Table, "orders")],
            restrictions: vec![],
        });
        model.explorer.select(ObjectId::new("table:orders"));

        let effects = update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTableData { .. })),
            "Enter opened the table: {effects:?}"
        );
        assert!(!model.active_document().kind.is_table());
        assert!(
            model
                .explorer
                .selected_node()
                .is_some_and(|node| node.expanded)
        );

        let effects = update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)),
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTableData { .. })),
            "o did not open the table: {effects:?}"
        );
        assert!(model.active_document().kind.is_table());
    }

    #[test]
    fn opening_data_on_something_that_is_not_a_table_only_says_so() {
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
        let documents = model.documents.len();

        let effects = update(&mut model, Action::OpenObjectData);

        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(model.documents.len(), documents);
        assert_eq!(
            model.messages.last().map(|entry| entry.message.as_str()),
            Some("Select a table or view to open its data.")
        );
    }

    /// Opening a table loads its metadata so `explorer.ddl` and Properties have something
    /// to show. It used to open the Properties overlay along with it -- harmless while the
    /// inspector was a pane, a modal over the grid once it became an overlay.
    #[test]
    fn opening_table_data_from_the_tree_skips_the_properties_modal() {
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
        let effects = update(&mut model, Action::OpenObjectData);
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
        assert!(
            !model.inspector.open,
            "opening a table popped the Properties overlay"
        );
        assert_eq!(model.focus, Focus::Results);

        // and the answer arriving later must not pop it either
        let generation = model.session_generation;
        let session = model.active_session.expect("session").0.to_string();
        update(
            &mut model,
            Action::InspectorLoaded {
                generation,
                session,
                qualified_name: "public.orders".into(),
                object: None,
                ddl: Some("create table orders ()".into()),
                dependencies: Vec::new(),
                dependents: Vec::new(),
                effective_privileges: Vec::new(),
                restrictions: Vec::new(),
            },
        );
        assert!(
            !model.inspector.open,
            "the inspector response opened the overlay on its own"
        );
        assert_eq!(
            model.inspector.ddl.as_deref(),
            Some("create table orders ()")
        );
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
