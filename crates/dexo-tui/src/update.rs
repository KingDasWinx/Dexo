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
                // dial that never finished. A warning from startup outranks it.
                match model
                    .startup_warning
                    .take_if(|(connection, _)| *connection == name)
                {
                    Some((_, warning)) => model.messages.warn(warning),
                    None => model.messages.info(format!("Connected to {name}")),
                }
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
            let menu_replay = model
                .pending_menu
                .take()
                .filter(|(waiting, _)| ready && *waiting == token)
                .map(|(_, invocation)| invocation);
            // An answer given for one connection never runs on another.
            if model.connection.name != name || model.active_session != session {
                model.run_prompt = None;
            }
            model.connection.name = name.clone();
            set_connection_driver(model, driver.clone());
            model.connection.ready = ready;
            model.connection.environment = environment;
            model.connection.read_only = read_only;
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
                if let Some(invocation) = menu_replay {
                    effects.extend(invoke_palette(model, invocation));
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
        Action::SessionCapabilities {
            session,
            unavailable,
        } => {
            model.unavailable.insert(session, unavailable);
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
                model.messages.info(message);
            }
            Vec::new()
        }
        Action::QueryResultSetFinished {
            key,
            index,
            rows_affected,
            truncated,
        } => {
            if let Some(tab) = result_tab_mut(model, &key, index) {
                tab.rows_affected = rows_affected;
                tab.truncated = truncated;
                tab.status = crate::model::OperationStatus::Finished;
            }
            Vec::new()
        }
        Action::ScriptFinished { key } => {
            model.active_task = None;
            model.active_query = None;
            model.active_operation = None;
            apply_sql_transactions(model, key.operation, usize::MAX);
            let mut effects = finish_schema_run(model, key.operation, None);
            if let Some((results, bars)) = document_output(model, &key)
                && results
                    .derived_backup
                    .as_ref()
                    .is_some_and(|backup| backup.operation == key.operation)
            {
                results.derived_backup = None;
                bars.good = bars.applied.clone();
            }
            // A run that went through answers with rows, so a pane left on Messages by the
            // previous error comes back to them.
            if operation_matches(model, &key) {
                model.results.view = crate::model::ResultsView::Grid;
            }
            effects.extend(persist_history_effect(model));
            effects
        }
        Action::QueryFailed {
            key,
            index,
            message,
            details,
            position,
        } => {
            model.active_task = None;
            model.active_query = None;
            model.active_operation = None;
            if let Some(tab) = result_tab_mut(model, &key, index) {
                tab.status = crate::model::OperationStatus::Failed;
            }
            apply_sql_transactions(model, key.operation, index);
            point_at_failure(model, &key, index, &message, position);
            // A sort or a clause the server would not run leaves the rows it had.
            let mut derived = false;
            if let Some((results, bars)) = document_output(model, &key)
                && let Some(backup) = results
                    .derived_backup
                    .take_if(|backup| backup.operation == key.operation)
            {
                results.tabs = backup.tabs;
                results.active = backup.active;
                bars.applied = bars.good.clone();
                bars.failed = true;
                derived = true;
            }
            // A run the user stopped did not fail: `error query cancelled` in red said it
            // had, and the grid has nothing to explain.
            if message.eq_ignore_ascii_case("query cancelled") {
                model.messages.info("Query cancelled.".into());
                return finish_schema_run(model, key.operation, Some(index));
            }
            // The statement that failed is Dexo's wrapper around the user's clause: its
            // position, and the line it quotes, mean nothing in what the user typed.
            let details = if derived {
                details
                    .into_iter()
                    .filter_map(|line| {
                        if line.starts_with("SQLSTATE") {
                            line.split(" · ").next().map(str::to_string)
                        } else if line.starts_with("DETAIL:") || line.starts_with("HINT:") {
                            Some(line)
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                details
            };
            model.messages.error_with(message, details);
            // The grid of a failed statement is empty; the reason is in Messages, so the
            // pane goes there and puts the new entry at the top. A clause that failed left
            // the rows it had, which stay on screen under the toast.
            if !derived && operation_matches(model, &key) {
                model.results.view = crate::model::ResultsView::Messages;
                model.results.messages_scroll =
                    u16::try_from(model.messages.newest_offset()).unwrap_or(u16::MAX);
            }
            finish_schema_run(model, key.operation, Some(index))
        }
        Action::CheckpointTick => {
            let mut effects = checkpoint_session(model);
            // An agent's write waiting for approval is said even with Agent Activity
            // closed; open, the screen reads the database on its own clock.
            if !model.mcp_audit.open {
                effects.push(Effect::CheckApprovals);
            }
            effects
        }
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
            let temporary = model.connections.is_temporary(&profile.name);
            model.secret_prompt =
                crate::screens::secret_prompt::SecretPrompt::open_for(purpose, profile, buffer);
            model.secret_prompt.temporary = temporary;
            Vec::new()
        }
        Action::SecretRejected {
            purpose,
            profile,
            buffer,
            keychain,
            message,
        } => {
            let temporary = model.connections.is_temporary(&profile.name);
            model.secret_prompt =
                crate::screens::secret_prompt::SecretPrompt::open_for(purpose, profile, buffer);
            model.secret_prompt.temporary = temporary;
            model.secret_prompt.keychain = keychain && !temporary;
            model.secret_prompt.error = Some(message);
            Vec::new()
        }
        Action::SubmitSecret { kind } => submit_secret(model, kind),
        Action::ConfirmDeleteProfile { decision } => confirm_delete(model, decision),
        Action::OpenConnections => open_connections(model),
        Action::DockerDiscovered(found) => {
            let screen = &mut model.connections;
            screen.docker = found;
            if screen.selected_profile >= screen.row_count() {
                screen.selected_profile = 0;
            }
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
                // Nothing saved to edit: editing a temporary connection is saving it.
                Some(profile) if model.connections.is_temporary(&profile.name) => {
                    return vec![Effect::RevealTemporarySecret {
                        profile: Box::new(profile),
                    }];
                }
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
        Action::OpenTemporaryConnection(profile) => open_temporary_connection(model, *profile),
        Action::SaveTemporaryConnection => {
            let selected = model
                .connections
                .selected()
                .filter(|profile| model.connections.is_temporary(&profile.name));
            match selected.or(model.connections.temporary.first()).cloned() {
                Some(profile) => vec![Effect::RevealTemporarySecret {
                    profile: Box::new(profile),
                }],
                None => {
                    model
                        .messages
                        .warn("No temporary connection to save.".into());
                    Vec::new()
                }
            }
        }
        Action::TemporarySaveForm { profile, .. }
            if profile.config.get("demo") == Some(&serde_json::Value::Bool(true)) =>
        {
            model.messages.warn(
                "The demo is rebuilt on every `dexo --demo`: there is nothing to keep. \
                 Copy its file and open that to keep a store."
                    .into(),
            );
            Vec::new()
        }
        Action::TemporarySaveForm { profile, password } => {
            let mut form = crate::screens::connection::ConnectionForm::open_edit(&profile);
            // A new profile with the temporary one's settings: saved through the normal
            // path, so the password goes to the keychain like any other.
            form.editing = None;
            form.saving_temporary = Some((*profile).clone());
            if let Some(password) = password {
                form.set_value("password", password.expose());
            }
            model.connection_form = form;
            Vec::new()
        }
        Action::DuplicateConnection
        | Action::MoveConnectionGroup { .. }
        | Action::DeleteConnection
            if model
                .connections
                .selected()
                .is_some_and(|profile| model.connections.is_temporary(&profile.name)) =>
        {
            model
                .messages
                .warn("This connection is temporary: save it first (Save Connection…).".into());
            Vec::new()
        }
        // Sessions are found by name: the copy must not be called what an open
        // temporary connection is, or it would take that session over.
        Action::DuplicateConnection => model
            .connections
            .selected()
            .map(|profile| Effect::DuplicateProfile {
                id: profile.id,
                taken: model
                    .connections
                    .temporary
                    .iter()
                    .map(|temporary| temporary.name.clone())
                    .collect(),
            })
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
            let mut effects = Vec::new();
            // The name this profile's sessions went by: the temporary connection the form
            // saved, or the profile itself before an edit renamed it.
            let mut previous_name = model
                .connections
                .profiles
                .iter()
                .find(|row| row.profile.id == profile.id)
                .map(|row| row.profile.name.clone());
            if let Some(temporary) = model.connection_form.saving_temporary.take() {
                // Its documents are bound to its id, and follow it to the saved one's.
                let (from, to) = (temporary.id.0.to_string(), profile.id.0.to_string());
                for document in &mut model.documents {
                    if document.connection_id.as_deref() == Some(from.as_str()) {
                        document.connection_id = Some(to.clone());
                    }
                }
                effects.push(flush_documents_effect(model));
                model
                    .connections
                    .temporary
                    .retain(|open| open.id != temporary.id);
                previous_name = Some(temporary.name);
                // Saving a temporary connection dials nothing, so nothing else closes it.
                model.connection_form.close();
            }
            // A connection the form created is saved now. The dial that follows may fail,
            // but that is a connection that failed to open, not a form that failed to save.
            if model.connection_form.open && model.connection_form.editing.is_none() {
                model.connection_form.close();
            }
            if let Some(from) = previous_name.filter(|from| *from != profile.name) {
                effects.extend(rename_sessions(model, &from, &profile.name));
            }
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
            effects
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
            // A test run from the form answers in the form, where the user is looking.
            if model.connection_form.open {
                if ok {
                    model
                        .connection_form
                        .set_notice(format!("{name}: the connection works"));
                } else {
                    model.connection_form.set_error(message);
                }
            } else if ok {
                model.messages.info(format!("{name} ok"));
            } else {
                model.messages.error(format!("{name}: {message}"));
            }
            Vec::new()
        }
        Action::SessionClosed { session } => {
            model.connections.remove_session(session);
            model.unavailable.remove(&session);
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
            model.palette.query.set_text(query);
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
            if let Some(bar) = focused_bar(model) {
                model.data.bars.input_mut(bar).insert_text(&text);
                return Vec::new();
            }
            // On the grid, the tree or the tabs every letter is a command: pasted text
            // would sort, count and filter at random.
            if crate::mouse::top_overlay(model).is_none()
                && model.effective_focus() != Focus::Editor
            {
                model.messages.info(
                    "Nothing here takes text; paste into the editor, a bar or a form.".into(),
                );
                return Vec::new();
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
        Action::ExplorerFirst => {
            move_sidebar_selection(model, i32::MIN / 2);
            Vec::new()
        }
        Action::ExplorerLast => {
            move_sidebar_selection(model, i32::MAX / 2);
            Vec::new()
        }
        Action::ExplorerPageUp => {
            move_sidebar_selection(model, -(explorer_visible_rows(model).max(2) as i32 - 1));
            Vec::new()
        }
        Action::ExplorerPageDown => {
            move_sidebar_selection(model, explorer_visible_rows(model).max(2) as i32 - 1);
            Vec::new()
        }
        Action::ExplorerCollapse => {
            let Some(id) = model.explorer.selected.clone() else {
                return Vec::new();
            };
            if model
                .explorer
                .selected_node()
                .is_some_and(|node| node.expanded)
            {
                model.explorer.collapse(&id);
            } else if let Some(parent) = model.explorer.parent_of(&id) {
                model.explorer.select(parent);
                model.explorer.sync_scroll(explorer_visible_rows(model));
            }
            Vec::new()
        }
        Action::ExplorerOpen => {
            let Some(node) = model.explorer.selected_node() else {
                return Vec::new();
            };
            if !node.expanded {
                return expand_selected_catalog(model);
            }
            if let Some(child) = node.children.first().map(|child| child.id.clone()) {
                model.explorer.select(child);
                model.explorer.sync_scroll(explorer_visible_rows(model));
            }
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
        // The same switch Ctrl+Tab makes: the session goes with the document. These moved
        // the tab only, and the header and the status bar kept the connection left behind.
        Action::NextDocumentTabFocus | Action::PrevDocumentTabFocus => {
            let step = if matches!(action, Action::NextDocumentTabFocus) {
                1
            } else {
                -1
            };
            model.advance_document_tab_focus(step);
            if model.nothing_open() {
                return Vec::new();
            }
            let index = model.active_document;
            activate_document(model, index)
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
        Action::ResolveCloseActive(choice) => {
            let document = model.active_document();
            model.close_prompt = Some(crate::model::ClosePrompt {
                document: document.id.clone(),
                title: document.title.clone(),
                choice,
            });
            resolve_close(model, choice)
        }
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
        Action::CountRows => count_rows(model),
        Action::RowsCounted { operation, result } => {
            use crate::screens::data::CountState;
            // The count is the document's, on screen or parked: one that ends while
            // another document is active lands in its own. An answer for a count since
            // cancelled is dropped.
            let running = |count: &Option<crate::screens::data::RowCount>| {
                count
                    .as_ref()
                    .is_some_and(|count| count.state == CountState::Running(operation))
            };
            let slot = if running(&model.data.count) {
                Some(&mut model.data.count)
            } else {
                let active = model.active_document;
                model
                    .documents
                    .iter_mut()
                    .enumerate()
                    .find(|(index, document)| *index != active && running(&document.browse.count))
                    .map(|(_, document)| &mut document.browse.count)
            };
            if let Some(count) = slot {
                match result {
                    Ok(rows) => {
                        if let Some(count) = count.as_mut() {
                            count.state = CountState::Exact(rows);
                        }
                    }
                    Err(message) => {
                        *count = None;
                        model.messages.error(format!("The count failed: {message}"));
                    }
                }
            }
            Vec::new()
        }
        Action::SortByColumn { column, add } => {
            let column = column.or_else(|| model.results.selection().map(|(_, col)| col));
            match column {
                Some(column) => sort_by_column(model, column, add),
                None => Vec::new(),
            }
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
            if at_page_edge(model, true) {
                return Vec::new();
            }
            let offset = model
                .data
                .page_offset
                .saturating_add(u64::from(model.data.page_limit));
            change_data_page(model, offset)
        }
        Action::PrevDataPage => {
            if at_page_edge(model, false) {
                return Vec::new();
            }
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
        Action::CycleTheme => cycle_theme(model, 1),
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
        Action::FocusClauseBar { bar } => {
            // From the log or the plan, the bars are on the grid: it comes back.
            if clause_bars_shown(model) && model.results.view != crate::model::ResultsView::Grid {
                model.results.view = crate::model::ResultsView::Grid;
            }
            if bars_drawn(model) {
                model.focus = Focus::Results;
                model.data.bars.focus = Some(bar);
            } else if clause_bars_shown(model) {
                model.messages.warn(
                    "WHERE and ORDER BY are on the grid; \\x goes back to it from the records."
                        .into(),
                );
            } else {
                model.messages.warn(
                    "WHERE and ORDER BY apply to a table's rows or a query's result; run one first."
                        .into(),
                );
            }
            Vec::new()
        }
        Action::DataPageLoaded {
            generation,
            session,
            ticket,
            page,
        } => {
            // Only the page this grid last asked for: an older one, or one asked for by
            // a document no longer on screen, would land in the wrong grid.
            if catalog_generation_matches(model, &session, generation)
                && model.data.page_ticket == Some(ticket)
            {
                model.data.bars.good = model.data.bars.applied.clone();
                model.data.apply_page(page.clone());
                // A page of the same columns -- sorted, filtered, turned -- keeps the
                // cursor's column: `s` again sorts the column it sorted.
                let column = model
                    .results
                    .selection()
                    .map(|(_, col)| col)
                    .filter(|_| model.results.columns() == page.columns.as_slice());
                model.results.clear();
                model.results.home_column = column.unwrap_or(0);
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
            ticket,
            message,
        } => {
            if generation == model.session_generation && model.data.page_ticket == Some(ticket) {
                model.data.loading = false;
                model.data.last_error = Some(message.clone());
                model.messages.error(message);
                // The next page asks with what last worked; the bars keep the text, drawn
                // as refused.
                model.data.bars.applied = model.data.bars.good.clone();
                model.data.bars.failed = true;
            }
            Vec::new()
        }
        Action::TableColumnsLoaded {
            generation,
            ticket,
            columns,
        } => {
            if generation == model.session_generation && model.data.columns_ticket == Some(ticket) {
                model.data.table = dexo_app::data::TableMeta::from_keys(columns);
                model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
                model.data.row_changes.clear();
            }
            Vec::new()
        }
        Action::TableColumnsFailed {
            generation,
            ticket,
            message,
        } => {
            if generation == model.session_generation && model.data.columns_ticket == Some(ticket) {
                model.messages.error(message);
            }
            Vec::new()
        }
        Action::ValueFetchFailed {
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
                show_value(
                    model,
                    crate::screens::value_viewer::view(&bytes_or_text(bytes)),
                );
            }
            Vec::new()
        }
        Action::MutationsApplied {
            generation,
            session,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                let applied = model.data.changes.pending().len();
                model.data.apply();
                model.data.row_changes.clear();
                // Done: the review has nothing left to show, and the user is told what
                // changed instead.
                model.data.review = None;
                model.messages.info(match applied {
                    1 => "Applied 1 change.".to_string(),
                    count => format!("Applied {count} changes."),
                });
                // The rows changed: a count of them no longer holds. The page they were
                // on is read again, filter and sort kept, not the first.
                let mut effects = drop_count(model);
                effects.extend(reload_object_data(model));
                return effects;
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
            names,
            effective_privileges,
            restrictions,
        } => {
            if catalog_generation_matches(model, &session, generation) {
                // A constraint, a function, a type or a group is not one the catalog hands
                // back by id: it is described by the node it was picked on, rather than
                // answered with "Select an object in Explorer.".
                let object = object.or_else(|| {
                    model.explorer.selected_node().map(|node| {
                        dexo_driver_api::CatalogObject::new(
                            node.id.clone(),
                            node.kind.clone(),
                            dexo_driver_api::QualifiedName::new(
                                None::<String>,
                                node.schema.clone(),
                                node.label.clone(),
                            ),
                            None,
                        )
                    })
                });
                if !qualified_name.is_empty() {
                    model.inspector.qualified_name = qualified_name;
                }
                model.inspector.object = object;
                model.inspector.ddl = ddl;
                model.inspector.dependencies = dependencies;
                model.inspector.dependents = dependents;
                model.inspector.names = names;
                model.inspector.effective_privileges = effective_privileges;
                model.inspector.restrictions = restrictions;
                model.inspector.error = None;
                model.inspector.note = None;
                model.inspector.editing_note = None;
                if let (Some(object), Some(connection_id)) =
                    (model.inspector.note_key(), active_connection_uuid(model))
                {
                    return vec![Effect::LoadNote {
                        connection_id,
                        object,
                    }];
                }
                // No note to read first: the editor the palette asked for opens now.
                if std::mem::take(&mut model.inspector.note_requested)
                    && model.inspector.object.is_some()
                {
                    start_note_editor(model);
                }
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
            model.messages.info(match model.data.copy_note.take() {
                Some(note) => note,
                // A name or a short value says what it was; a long one only that it went.
                None if lines == 1 && !text.is_empty() && text.chars().count() <= 60 => {
                    format!("copied {text} to clipboard")
                }
                None if lines == 1 => "copied to clipboard".into(),
                None => format!("copied {lines} lines to clipboard"),
            });
            model.explorer.copied = Some(text.clone());
            model.data.clipboard = text;
            Vec::new()
        }
        Action::ClipboardFailed { message } => {
            model.data.copy_note = None;
            model.messages.error(message);
            Vec::new()
        }
        // Over SSH, in a container or with no display there is no clipboard to read, yet
        // a copy still reaches the terminal. Paste what Dexo copied last, as an editor's
        // own register would, instead of failing with the backend's error.
        Action::ClipboardUnreadable => {
            if model.data.clipboard.is_empty() {
                model.messages.warn(
                    "The system clipboard cannot be read here. Paste with the terminal's own shortcut, or copy in Dexo first."
                        .into(),
                );
                Vec::new()
            } else {
                let text = model.data.clipboard.clone();
                update(model, Action::Paste(text))
            }
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
            // The same answer the palette gives -- from a key too -- instead of an empty
            // dialog.
            if model.data.changes.pending().is_empty() {
                model.messages.warn("No pending changes.".into());
                return Vec::new();
            }
            // The review asks on production only if it knows it is on production, and
            // nothing outside a test told it.
            model.data.environment =
                dexo_app::Environment::parse_strict(&model.connection.environment);
            model.data.open_review();
            Vec::new()
        }
        Action::SubmitTransfer => run_transfer(model),
        Action::ApplyChanges => apply_changes(model),
        Action::FailApply => {
            model.data.fail_apply("apply failed".into());
            Vec::new()
        }
        Action::RevertChanges => {
            discard_all_pending(model);
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
            if !edits_refused(model, "Insert") {
                let columns = model.results.columns().to_vec();
                model.data.insert_form.open_for(&model.data.table, &columns);
            }
            Vec::new()
        }
        Action::CancelInsertRow => {
            model.data.insert_form.close();
            Vec::new()
        }
        Action::SubmitInsertRow => submit_insert_row(model),
        Action::EditCell => open_cell_edit(model),
        Action::InspectValue => inspect_selected(model),
        Action::OpenRelatedPicker => open_related_picker(model),
        Action::NoteLoaded { object, note } => {
            if model.inspector.note_key().as_deref() == Some(object.as_str()) {
                model.inspector.note = note;
                if std::mem::take(&mut model.inspector.note_requested) {
                    start_note_editor(model);
                }
            }
            Vec::new()
        }
        Action::NoteSaved { object, saved } => {
            match saved {
                Ok(note) => {
                    model.messages.info(match &note {
                        Some(_) => format!("Saved the note on {object}."),
                        None => format!("Removed the note on {object}."),
                    });
                    if model.inspector.note_key().as_deref() == Some(object.as_str()) {
                        model.inspector.note = note;
                    }
                }
                Err(error) => model
                    .messages
                    .error(format!("The note on {object} was not saved: {error}")),
            }
            Vec::new()
        }
        Action::EditObjectNote => edit_object_note(model),
        Action::OpenSaveQuery => open_save_query(model),
        Action::OpenSavedQueries => {
            if model.project_id.is_empty() {
                model
                    .messages
                    .warn("Saved queries belong to a project; open one first.".into());
                return Vec::new();
            }
            model.saved_queries = crate::screens::saved_queries::SavedQueriesPicker {
                open: true,
                ..Default::default()
            };
            vec![Effect::LoadSavedQueries {
                project_id: model.project_id.clone(),
            }]
        }
        Action::SavedQueriesLoaded(listed) => {
            let picker = &mut model.saved_queries;
            match listed {
                Ok(items) => {
                    picker.set_items(items);
                    picker.renaming = None;
                    picker.deleting = None;
                    picker.clamp();
                }
                Err(message) => picker.error = Some(message),
            }
            Vec::new()
        }
        Action::SavedQueryDone(done) => {
            match done {
                Ok(message) => {
                    model.saved_queries.error = None;
                    model.messages.info(message);
                }
                Err(message) => {
                    if model.saved_queries.open {
                        model.saved_queries.error = Some(message.clone());
                    }
                    model.messages.error(message);
                }
            }
            Vec::new()
        }
        Action::ForeignKeysLoaded {
            generation,
            table,
            result,
        } => {
            let current = model
                .data
                .related_picker
                .as_ref()
                .is_some_and(|picker| picker.table == table && picker.links.is_none());
            if !current {
                return Vec::new();
            }
            // Asked of a session that has since been replaced: nothing will answer now.
            if generation != model.session_generation {
                model.data.related_picker = None;
                model
                    .messages
                    .warn("The connection changed while the keys were read; press f again.".into());
                return Vec::new();
            }
            match result {
                Ok(keys) => {
                    let dialect = crate::screens::editor::editor_dialect(model);
                    let links = related_links(&table, &keys, dialect);
                    if links.is_empty() {
                        model.data.related_picker = None;
                        model.messages.info(format!(
                            "No foreign key leads from or to {}.",
                            table.display_unquoted()
                        ));
                    } else if let Some(picker) = &mut model.data.related_picker {
                        picker.links = Some(links);
                    }
                }
                Err(message) => {
                    model.data.related_picker = None;
                    model
                        .messages
                        .error(format!("Could not list the foreign keys: {message}"));
                }
            }
            Vec::new()
        }
        Action::DataNavBack => data_nav_back(model),
        Action::OpenDdlPreview => execute_on_document_connection(model, action),
        Action::ConfirmDdl => {
            model.schema_editor.confirm_typed();
            Vec::new()
        }
        Action::ApplyDdl => apply_ddl(model),
        Action::ApplyRawDdl => {
            let sql = model.active_document().text();
            if !sql.trim().is_empty() {
                model.schema_editor.apply_raw(sql);
                model.schema_editor.errors.clear();
                model.schema_editor.footer = crate::widgets::form::FooterFocus::Submit;
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
        Action::SchemaDiffOpenScript => crate::screens::schema_diff::open_script(model),
        Action::SchemaSourcesLoaded(snapshots) => {
            use crate::screens::schema_diff::{DiffOption, DiffOptionKind};
            if model.schema_diff.open && model.schema_diff.source_prompt {
                model.schema_diff.add_snapshots(
                    snapshots
                        .into_iter()
                        .map(|(name, driver)| DiffOption {
                            label: format!("snapshot {name}  ({driver})"),
                            name,
                            kind: DiffOptionKind::Snapshot,
                            driver,
                        })
                        .collect(),
                );
            }
            Vec::new()
        }
        Action::SchemaDiffLoaded {
            from_label,
            to_label,
            ordered,
        } => {
            // The sources stay for another look, with the result over them.
            let from_connection = model.schema_diff.from_connection.take();
            model.schema_diff = crate::screens::schema_diff::SchemaDiffScreen::from_ordered(
                from_label, to_label, &ordered,
            );
            model.schema_diff.from_connection = from_connection;
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
        Action::OpenExplain => execute_on_document_connection(model, action),
        Action::DismissToast => {
            model.messages.dismiss();
            Vec::new()
        }
        Action::ToastTick => {
            model.messages.tick();
            Vec::new()
        }
        Action::DiagnosticsTick => {
            crate::screens::editor::settle_diagnostics(model);
            Vec::new()
        }
        Action::CycleResultsView => {
            // One flat ring over everything the output pane can show, so the user has a
            // single question to answer instead of two nested ones.
            use crate::model::ResultsView;
            use crate::screens::explain::ExplainView;
            model.results.explain_scroll = 0;
            model.results.messages_scroll = 0;
            match (model.results.view, model.results.explain.view) {
                // No plan to show: Explain is not a stop on the way, and the log is one
                // press from the grid instead of four.
                (ResultsView::Grid, _) if model.results.explain.plan.is_none() => {
                    model.results.view = ResultsView::Messages;
                }
                (ResultsView::Grid, _) => {
                    model.results.view = ResultsView::Explain;
                    model.results.explain.view = ExplainView::Tree;
                }
                (ResultsView::Explain, ExplainView::Summary) => {
                    model.results.view = ResultsView::Messages;
                }
                (ResultsView::Explain, view) => model.results.explain.view = view.next(),
                (ResultsView::Messages, _) => model.results.view = ResultsView::Grid,
            }
            scroll_results_view_to_start(model);
            Vec::new()
        }
        // ANALYZE runs the statement, so it asks first; it used to run straight from the
        // palette, and its "confirmation" was a flag it set on itself.
        Action::ConfirmExplainAnalyze => {
            if let Some(reason) =
                model.unavailable_reason(dexo_driver_api::Capability::ExplainAnalyze)
            {
                model.messages.warn(reason.to_string());
            } else if analyzed_write(model).is_some() && on_production(model) {
                // The name typed is the confirmation; a second dialog before it would
                // only be one more Enter to press out of habit.
                return update(model, Action::RunExplainAnalyze);
            } else if !analyze_refused(model) {
                model.explain_prompt = Some(crate::widgets::form::FooterFocus::Submit);
            }
            Vec::new()
        }
        Action::RunExplainAnalyze => {
            model.explain_prompt = None;
            if let Some(statement) = analyzed_write(model) {
                let what = vec![
                    "EXPLAIN ANALYZE runs this statement, then rolls back what it changed:"
                        .to_string(),
                    format!("  {}", statement.lines().next().unwrap_or_default()),
                ];
                if !production_cleared(model, what, Action::RunExplainAnalyze) {
                    return Vec::new();
                }
            }
            execute_on_document_connection(model, action)
        }
        Action::OpenTryIndex => {
            if model.results.explain.plan.is_none() {
                model.messages.warn(
                    "Try index compares with the statement's plan: explain it first (F7).".into(),
                );
            } else {
                model.try_index = Some(crate::screens::explain::TryIndexPrompt::default());
            }
            Vec::new()
        }
        Action::TryIndex { .. } => {
            model.try_index = None;
            execute_on_document_connection(model, action)
        }
        Action::OpenAdmin => {
            model.admin.open = true;
            model.admin.selected = 0;
            model.admin.terminate = None;
            model.admin.last_error = None;
            load_admin_sessions(model)
        }
        Action::AdminTerminated { result } => {
            match result {
                Ok(message) => {
                    model.admin.last_error = None;
                    model.messages.info(message);
                }
                Err(message) => {
                    model.admin.last_error = Some(format!("Not terminated: {message}"));
                    model.messages.error(message);
                }
            }
            load_admin_sessions(model)
        }
        Action::OpenMcpProfiles => {
            let screen = &mut model.mcp_profiles;
            screen.open = true;
            screen.status.clear();
            screen.confirm = None;
            screen.detail_scroll = 0;
            screen.grant_from_palette = false;
            vec![Effect::LoadMcpProfiles]
        }
        Action::OpenMcpGrantForm => {
            // Asked for from the palette, with the profiles not read yet: they are read,
            // and the form opens when they arrive.
            if !model.mcp_profiles.open {
                model.mcp_profiles.grant_from_palette = true;
                model.mcp_profiles.grant_when_loaded = true;
                model.mcp_profiles.open = true;
                return vec![Effect::LoadMcpProfiles];
            }
            open_grant_form(model);
            Vec::new()
        }
        Action::McpGrantCreated { message } => {
            model.mcp_profiles.grant_form = None;
            model.mcp_profiles.status = message.clone();
            if model.mcp_profiles.grant_from_palette {
                model.mcp_profiles.open = false;
                model.mcp_profiles.grant_from_palette = false;
            }
            model.messages.info(message);
            vec![Effect::LoadMcpProfiles]
        }
        Action::McpGrantFailed { message } => {
            match model.mcp_profiles.grant_form.as_mut() {
                Some(form) => form.error = Some(message),
                None => model.messages.error(message),
            }
            Vec::new()
        }
        Action::ToggleMcpProfile => match model.mcp_profiles.toggle_selected() {
            Some(enabled) => vec![Effect::SetMcpProfileEnabled {
                name: model.mcp_profiles.name.clone(),
                enabled,
            }],
            None => Vec::new(),
        },
        Action::RevokeProfileGrants => {
            model.mcp_profiles.ask_revoke_profile();
            Vec::new()
        }
        Action::RevokeAllMcpGrants => {
            // From the palette or Agent Activity: the profiles, read again, with the
            // question of how many grants go.
            model.mcp_audit.open = false;
            model.mcp_profiles.open = true;
            model.mcp_profiles.grant_from_palette = false;
            model.mcp_profiles.revoke_all_when_loaded = true;
            vec![Effect::LoadMcpProfiles]
        }
        Action::McpGrantsRevoked { count } => {
            model.mcp_profiles.confirm = None;
            model.mcp_profiles.status = match count {
                0 => "There was nothing to revoke.".to_string(),
                1 => "Revoked 1 grant.".to_string(),
                n => format!("Revoked {n} grants."),
            };
            Vec::new()
        }
        Action::McpRevokeFailed { message } => {
            model.mcp_profiles.status = message;
            Vec::new()
        }
        Action::McpProfileDeleted { name } => {
            model.mcp_profiles.status = format!("Deleted the profile {name}.");
            Vec::new()
        }
        Action::OpenSettings => {
            model.settings.open = true;
            model.settings.focus = 0;
            model.settings.confirm_reset = false;
            // A theme file dropped in, changed or removed since the start shows as it is.
            if let Ok(paths) = dexo_storage::AppPaths::discover() {
                load_user_themes(model, &paths.data_dir);
            }
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
                model.catalog_complete = true;
                // The underlines can now say which tables and columns do not exist.
                let sql = model.active_document().text();
                let cursor = model.active_document().byte_cursor();
                crate::screens::editor::refresh_diagnostics(model, &sql, cursor);
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
        Action::Notice(message) => {
            model.messages.info(message);
            Vec::new()
        }
        Action::ToggleRecordView | Action::SetRecordView(_) => {
            model.expanded_records = match &action {
                Action::SetRecordView(on) => *on,
                _ => !model.expanded_records,
            };
            record_follow_field(model);
            model.messages.info(
                if model.expanded_records {
                    "Expanded display is on: one field per line."
                } else {
                    "Expanded display is off."
                }
                .into(),
            );
            Vec::new()
        }
        Action::OpenFind { replace } => {
            let doc = model.active_document();
            if doc.kind.is_table() || doc.kind.is_placeholder() {
                model
                    .messages
                    .warn("Find searches a SQL document; open one first.".into());
            } else {
                model.focus = Focus::Editor;
                crate::screens::find::open(model, replace);
            }
            Vec::new()
        }
        Action::EditorToggleComment
        | Action::EditorDuplicateLine
        | Action::EditorMoveLine { .. }
            if model.active_document().kind.is_table()
                || model.active_document().kind.is_placeholder() =>
        {
            Vec::new()
        }
        Action::EditExternally => {
            let doc = model.active_document();
            if doc.kind.is_table() || doc.kind.is_placeholder() {
                model
                    .messages
                    .warn("Only a SQL document opens in an external editor.".into());
            } else {
                crate::screens::editor::end_typing(model);
                let doc = model.active_document();
                model.external_edit = Some(crate::model::ExternalEdit {
                    document: doc.id.clone(),
                    text: doc.text(),
                });
            }
            Vec::new()
        }
        Action::ExternalEditFinished { document, text } => {
            if document == CELL_EDIT_DOCUMENT {
                finish_cell_edit_externally(model, text);
                return Vec::new();
            }
            match text {
                Ok(text) => crate::screens::editor::replace_document(model, &document, &text),
                Err(message) => model.messages.error(message),
            }
            Vec::new()
        }
        Action::EditorToggleComment => {
            crate::screens::editor::toggle_comment(model);
            Vec::new()
        }
        Action::EditorDuplicateLine => {
            crate::screens::editor::duplicate_lines(model);
            Vec::new()
        }
        Action::EditorMoveLine { up } => {
            crate::screens::editor::move_lines(model, up);
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
            model.editor.history_confirm_clear = false;
            model.editor.history_selected = 0;
            model.editor.history_search.clear();
            // This connection's statements; with none connected, all of them.
            vec![Effect::LoadHistory {
                connection_id: (!model.connection.name.is_empty())
                    .then(|| model.connection.name.clone()),
            }]
        }
        Action::ClearHistory => confirm_clear_history(model),
        Action::HistoryLoaded(mut entries) => {
            // Newest first, so keeping the first of each statement keeps the latest run
            // of it: the same query run ten times is one row.
            let mut seen = std::collections::HashSet::new();
            entries.retain(|sql| seen.insert(sql.clone()));
            model.editor.history = entries;
            model.editor.history_selected = 0;
            Vec::new()
        }
        Action::HistoryPick => open_history_entry(model),
        Action::SnippetsLoaded(mut snippets) => {
            model.editor.snippet_pending = false;
            // The built-in ones follow the person's own, which win on a name.
            for builtin in dexo_sql::builtin_snippets() {
                if !snippets.iter().any(|own| own.name == builtin.name) {
                    snippets.push(builtin);
                }
            }
            model.editor.snippets = snippets;
            model.editor.snippet_open = true;
            model.editor.snippet_selected = 0;
            Vec::new()
        }
        Action::SnippetPick => {
            crate::screens::editor::insert_snippet_at(model, model.editor.snippet_selected);
            Vec::new()
        }
        Action::DdlPreviewed {
            statements,
            confirmation,
            warnings,
            risk,
        } => {
            let preview = dexo_app::schema::DdlPreview {
                plan: {
                    let mut plan = dexo_driver_api::DdlPlan::default();
                    for sql in statements {
                        plan.push(sql, false);
                    }
                    plan.warnings = warnings;
                    plan
                },
                risk,
                dependents: Vec::new(),
                grants: Vec::new(),
                confirmation,
                warnings: Vec::new(),
            };
            let (origin, change) = match model.schema_editor.pending.take() {
                Some((origin, change)) => (origin, Some(change)),
                None => (crate::screens::schema_editor::PreviewOrigin::Form, None),
            };
            model
                .schema_editor
                .open_preview_for(preview, origin, change);
            Vec::new()
        }
        Action::SchemaApplied {
            message,
            refresh,
            ok,
        } => {
            if ok {
                model.messages.info(message);
            } else {
                model.messages.warn(message);
            }
            model.schema_editor.preview = None;
            model.schema_editor.applying = None;
            refresh
                .map(|target| refresh_after_schema_change(model, &target))
                .unwrap_or_default()
        }
        Action::SchemaFailed { message } => {
            model.messages.error(message);
            // A change that failed from the form is fixed in the form, whose fields are
            // as they were.
            if model.schema_editor.applying.take()
                == Some(crate::screens::schema_editor::PreviewOrigin::Form)
                && model.schema_editor.preview.is_none()
            {
                model.schema_editor.open = true;
                model.schema_editor.footer = crate::widgets::form::FooterFocus::Input;
            }
            Vec::new()
        }
        Action::ExplainLoaded {
            plan,
            sql,
            indexes,
            document,
            operation,
        } => {
            if model.active_operation == Some(operation) {
                model.active_operation = None;
            }
            if let Some(results) = results_of_document(model, &document) {
                results.explain.set_plan(*plan, sql, indexes);
                // Show the plan where output lives; never move the user's focus for it.
                results.view = crate::model::ResultsView::Explain;
                results.explain_scroll = 0;
            }
            Vec::new()
        }
        Action::ExplainFailed {
            document,
            operation,
            message,
        } => {
            if model.active_operation == Some(operation) {
                model.active_operation = None;
            }
            model.messages.error(message);
            let newest = u16::try_from(model.messages.newest_offset()).unwrap_or(u16::MAX);
            if let Some(results) = results_of_document(model, &document) {
                // The plan on screen belonged to an earlier statement; left there, it
                // read as the answer for this one.
                results.explain.clear();
                results.view = crate::model::ResultsView::Messages;
                results.messages_scroll = newest;
            }
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
            model.admin.selected = model
                .admin
                .selected
                .min(model.admin.sessions.len().saturating_sub(1));
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
            if std::mem::take(&mut model.mcp_profiles.grant_when_loaded) {
                open_grant_form(model);
            }
            if std::mem::take(&mut model.mcp_profiles.revoke_all_when_loaded) {
                model.mcp_profiles.ask_revoke_all();
            }
            Vec::new()
        }
        Action::McpAuditLoaded {
            events,
            pending,
            now,
        } => {
            model.mcp_audit.events = events;
            // A request decided elsewhere, or out of time, takes its confirmation away.
            if model.mcp_audit.load(pending, now) {
                model.messages.warn(
                    "That request was decided elsewhere or ran out of time; nothing was settled."
                        .into(),
                );
            }
            Vec::new()
        }
        Action::AgentActivityTick => {
            let mut effects = Vec::new();
            if model.mcp_audit.open {
                effects.push(Effect::LoadMcpAudit);
            }
            // The profiles follow what `dexo mcp` changes while the screen is open, but
            // not under a form or a question being answered.
            let profiles = &model.mcp_profiles;
            if profiles.open && profiles.grant_form.is_none() && profiles.confirm.is_none() {
                effects.push(Effect::LoadMcpProfiles);
            }
            effects
        }
        Action::ApprovalsWaiting(pending) => {
            // Said once per request, and not while the screen that lists them is open.
            let new = pending
                .iter()
                .filter(|request| !model.mcp_audit.announced.contains(&request.id))
                .count();
            if new > 0 && !model.mcp_audit.open {
                let key = crate::palette::shortcut_for(model, "mcp.audit", None)
                    .map(|key| format!(" ({key})"))
                    .unwrap_or_default();
                model.messages.warn(format!(
                    "An agent's write is waiting for your approval in Agent Activity{key}."
                ));
            }
            model.mcp_audit.announced = pending.iter().map(|request| request.id).collect();
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
        Action::DocumentLoadFailed { document, message } => {
            // The tab was opened for the file before it was read. With nothing read
            // into it, it is only a way to write over a file Dexo could not open.
            if let Some(index) = model
                .documents
                .iter()
                .position(|candidate| candidate.id == document)
            {
                model.messages.error(message);
                return remove_document(model, index);
            }
            model.messages.error(message);
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
                    effects.extend(remove_document(model, index));
                }
            }
            effects
        }
        Action::DocumentSaveFailed { document, message } => {
            // A close armed by `:wq` or the unsaved-changes prompt was waiting on this
            // write; left armed, a later save would close the tab by surprise.
            if model
                .pending_document_close
                .as_ref()
                .is_some_and(|pending| pending.document == document)
            {
                model.pending_document_close = None;
            }
            model
                .messages
                .error(format!("The file was not saved: {message}"));
            Vec::new()
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
                    model.results.explain_scroll = model.hits.scroll(
                        crate::mouse::ScrollArea::Explain,
                        model.results.explain_scroll,
                        -1,
                    );
                }
                crate::model::ResultsView::Messages => {
                    model.results.messages_scroll = model.hits.scroll(
                        crate::mouse::ScrollArea::Messages,
                        model.results.messages_scroll,
                        -1,
                    );
                }
                crate::model::ResultsView::Grid if record_view_shown(model) => {
                    record_move_field(model, -1)
                }
                crate::model::ResultsView::Grid => model.results.move_cursor_row(-1, false),
            }
            Vec::new()
        }
        Action::ResultsDown => {
            match model.results.view {
                crate::model::ResultsView::Explain => {
                    model.results.explain_scroll = model.hits.scroll(
                        crate::mouse::ScrollArea::Explain,
                        model.results.explain_scroll,
                        1,
                    );
                }
                crate::model::ResultsView::Messages => {
                    // Bounded by what the last frame drew: the log is the one list here
                    // that only grows, and an entry can span several rows.
                    model.results.messages_scroll = model.hits.scroll(
                        crate::mouse::ScrollArea::Messages,
                        model.results.messages_scroll,
                        1,
                    );
                }
                crate::model::ResultsView::Grid if record_view_shown(model) => {
                    record_move_field(model, 1)
                }
                crate::model::ResultsView::Grid => model.results.move_cursor_row(1, false),
            }
            Vec::new()
        }
        // On the plan or the log the same keys move the text: a page, the start, the end.
        Action::ResultsPageUp
        | Action::ResultsPageDown
        | Action::ResultsTop
        | Action::ResultsBottom
        | Action::ResultsFirstColumn
        | Action::ResultsLastColumn
            if model.results.view != crate::model::ResultsView::Grid =>
        {
            let page = (record_capacity(model) as i32 - 1).max(1);
            scroll_output_view(
                model,
                match action {
                    Action::ResultsPageUp => -page,
                    Action::ResultsPageDown => page,
                    Action::ResultsTop | Action::ResultsFirstColumn => i32::MIN / 2,
                    _ => i32::MAX / 2,
                },
            );
            Vec::new()
        }
        // In the record view the fields run down the screen and the records side by side:
        // Left and Right turn records, Up and Down walk the fields.
        Action::ResultsLeft if record_view_shown(model) => {
            model.results.move_cursor_row(-1, false);
            model.results.record_scroll = 0;
            record_follow_field(model);
            Vec::new()
        }
        Action::ResultsRight if record_view_shown(model) => {
            model.results.move_cursor_row(1, false);
            model.results.record_scroll = 0;
            record_follow_field(model);
            Vec::new()
        }
        Action::ResultsLeft => {
            model.results.move_cursor_col(-1);
            Vec::new()
        }
        Action::ResultsRight => {
            model.results.move_cursor_col(1);
            Vec::new()
        }
        Action::ResultsPageUp if record_view_shown(model) => {
            record_move_field(model, -(record_capacity(model) as i32 - 1).max(1));
            Vec::new()
        }
        Action::ResultsPageDown if record_view_shown(model) => {
            record_move_field(model, (record_capacity(model) as i32 - 1).max(1));
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
            model.results.record_scroll = 0;
            record_follow_field(model);
            Vec::new()
        }
        Action::ResultsCollapse => {
            model.results.picked_rows.clear();
            if let Some((row, col)) = model.results.selection() {
                model.results.select_cell(row, col);
            }
            Vec::new()
        }
        // As far as there are rows: the move clamps at the last.
        Action::ResultsBottom => {
            let rows = i32::try_from(model.results.row_count()).unwrap_or(i32::MAX);
            model.results.move_cursor_row(rows, false);
            model.results.record_scroll = 0;
            record_follow_field(model);
            Vec::new()
        }
        Action::ResultsFirstColumn | Action::ResultsLastColumn if record_view_shown(model) => {
            let last = model.results.columns().len().saturating_sub(1);
            let row = model.results.cursor_row().unwrap_or(0);
            let col = if matches!(action, Action::ResultsFirstColumn) {
                0
            } else {
                last
            };
            model.results.select_cell(row, col);
            record_follow_field(model);
            Vec::new()
        }
        Action::ResultsFirstColumn | Action::ResultsLastColumn => {
            let columns = i32::try_from(model.results.columns().len()).unwrap_or(i32::MAX);
            model
                .results
                .move_cursor_col(if matches!(action, Action::ResultsFirstColumn) {
                    -columns
                } else {
                    columns
                });
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
            model.layout_dirty = true;
            model.sync_grid_viewport();
            Vec::new()
        }
        Action::HideResults => {
            model.panes.results_visible = !model.panes.results_visible;
            if !model.panes.results_visible && model.focus == Focus::Results {
                model.focus = Focus::Editor;
            }
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
            // Documents are kept for recovery whatever happens; an open transaction and
            // grid edits are not, so quitting asks first when there are any.
            if model.quit_prompt.is_none() && !quit_losses(model).is_empty() {
                model.quit_prompt = Some(crate::widgets::form::FooterFocus::Cancel);
                return Vec::new();
            }
            model.quit_prompt = None;
            // The server keeps a statement running after its client is gone: it is asked
            // to stop first.
            let mut effects: Vec<Effect> = model
                .active_query
                .and(model.active_operation)
                .map(Effect::CancelOperation)
                .into_iter()
                .collect();
            effects.extend(checkpoint_dirty(model));
            effects.push(flush_documents_effect(model));
            effects.push(persist_layout_effect(model));
            effects.push(Effect::Shutdown);
            effects
        }
        Action::OpenProjects => open_projects(model, None),
        Action::SwitchProject { name } => switch_project(model, name),
        Action::ProjectSwitchTarget(project) => start_switch(model, project),
        Action::CreateProject { name } => {
            if model.projects.list.iter().any(|other| other.name == name) {
                model.projects.error = Some(format!("a project named {name} exists already"));
                return Vec::new();
            }
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.name_input.clear();
            // Asked for by the palette, the dialog was the question; asked for from the
            // list, the answer is the list with the new project in it.
            if !model.projects.from_list {
                model.projects.open = false;
            }
            model.messages.info(format!("created project {name}"));
            vec![Effect::CreateProject { name }]
        }
        Action::RenameProject { name } => {
            let Some(project) = model.projects.selected().cloned() else {
                return Vec::new();
            };
            if model
                .projects
                .list
                .iter()
                .any(|other| other.name == name && other.id != project.id)
            {
                model.projects.error = Some(format!("a project named {name} exists already"));
                return Vec::new();
            }
            // Renamed, the dialog is the list again, or gone when it was only asked to
            // rename: it stayed on the name just typed.
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.name_input.clear();
            model.projects.error = None;
            if model.projects.intent == Some(crate::screens::projects::ProjectIntent::Rename) {
                model.projects.open = false;
                model.projects.intent = None;
            }
            vec![Effect::RenameProject {
                id: project.id.0.to_string(),
                name,
            }]
        }
        Action::DeleteProject => {
            if model.projects.list.len() <= 1 {
                model.projects.error = Some(
                    "Dexo keeps at least one project: create another before deleting this one."
                        .into(),
                );
                return Vec::new();
            }
            model
                .projects
                .selected()
                .map(|project| Effect::PreviewProjectDelete {
                    id: project.id.0.to_string(),
                })
                .into_iter()
                .collect()
        }
        Action::ConfirmProjectDelete => confirm_project_delete(model),
        Action::ConfirmSwitchDirty => {
            resolve_project_switch(model, crate::model::CloseChoice::Save)
        }
        Action::ResolveProjectSwitch(choice) => resolve_project_switch(model, choice),
        Action::CancelProjectSwitch => {
            model.projects.pending = None;
            model.projects.dirty_choice = None;
            Vec::new()
        }
        Action::ProjectsLoaded(projects) => {
            model.projects.load(projects);
            // A rename changes the name of the project that is open, which the header
            // shows: it kept the old one.
            if let Some(open) = model
                .projects
                .list
                .iter()
                .find(|project| project.id.0.to_string() == model.project_id)
            {
                model.project = open.name.clone();
                let name = open.name.clone();
                if !model.projects.recents.contains(&name) {
                    model.projects.touch_recent(&name);
                }
            }
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
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.recents.retain(|item| item != &name);
            model.messages.info(format!("deleted project {name}"));
            if model.project != name {
                return Vec::new();
            }
            // The project that was open is gone. What was in it goes with it, and the
            // workbench moves to another, as a switch does -- but without saving a
            // single document into a project that no longer exists.
            let fallback = model
                .projects
                .list
                .iter()
                .find(|project| project.name != name)
                .cloned();
            model.documents = vec![crate::model::EditorDocument::placeholder()];
            model.active_document = 0;
            model.project.clear();
            model.project_id.clear();
            match fallback {
                Some(target) => {
                    model.projects.closing_sessions = model.connections.sessions.len();
                    let switch = crate::runtime::project_manager::ProjectSwitch {
                        stage: crate::runtime::project_manager::ProjectSwitchStage::CloseProjectSessions,
                        target,
                        operation: crate::runtime::OperationId::new(),
                    };
                    model.projects.pending = Some(switch.clone());
                    crate::runtime::project_manager::advance(model, &switch)
                }
                None => vec![Effect::ListProjects],
            }
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
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            model.projects.mode = crate::screens::projects::ProjectsMode::DeleteConfirm;
            model.projects.delete = Some(crate::screens::projects::ProjectDeletePrompt {
                project,
                preview,
                delete_connections: false,
                typed: Default::default(),
            });
            Vec::new()
        }
        Action::OpenConfigTransfer => {
            // Opened fresh: the last import's clashes and the last path are not carried in.
            model.config_transfer.reset();
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Export;
            Vec::new()
        }
        Action::ExportConfig { path } => {
            // A file that is there is only replaced when the person says so.
            if path.exists() && model.config_transfer.overwrite.as_ref() != Some(&path) {
                model.config_transfer.overwrite = Some(path);
                model.config_transfer.focus = 1;
                return Vec::new();
            }
            model.config_transfer.overwrite = None;
            model.config_transfer.focus = 0;
            model.config_transfer.message = None;
            model.config_transfer.path = path.clone();
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Export;
            vec![Effect::ExportConfig { path }]
        }
        Action::ConfigExported => {
            model.config_transfer.message = Some(format!(
                "Exported {} and {} to {}.",
                match model.connections.profiles.len() {
                    1 => "1 connection".to_string(),
                    n => format!("{n} connections"),
                },
                match model.projects.list.len() {
                    1 => "1 project".to_string(),
                    n => format!("{n} projects"),
                },
                model.config_transfer.path.display()
            ));
            Vec::new()
        }
        Action::ImportConfig { path } => {
            model.config_transfer.path = path.clone();
            model.config_transfer.mode =
                crate::screens::config_transfer::ConfigTransferMode::Import;
            model.config_transfer.message = None;
            vec![Effect::ImportConfig { path }]
        }
        Action::ApplyConfigImport => {
            // An imported connection called what an open temporary one is would take
            // its session over, as sessions are found by name.
            let clash = model.config_transfer.preview.as_ref().and_then(|preview| {
                preview
                    .incoming
                    .iter()
                    .map(|name| match model.config_transfer.resolutions.get(name) {
                        Some(dexo_storage::ImportResolution::Rename(renamed)) => renamed,
                        _ => name,
                    })
                    .find(|name| model.connections.is_temporary(name))
                    .cloned()
            });
            if let Some(name) = clash {
                model.config_transfer.message = Some(format!(
                    "{name} is the name of an open temporary connection; rename it on import (r), or save or close that one first"
                ));
                return Vec::new();
            }
            let path = model.config_transfer.path.clone();
            let resolutions = model.config_transfer.resolutions.clone();
            vec![Effect::ApplyConfigImport { path, resolutions }]
        }
        Action::ConfigPreviewed(preview) => {
            let screen = &mut model.config_transfer;
            screen.resolutions.clear();
            screen.selected = 0;
            screen.scroll = 0;
            screen.focus = 0;
            screen.needing_secret.clear();
            screen.commands.clear();
            screen.preview = Some(preview);
            Vec::new()
        }
        Action::ConfigImported {
            needing_secret,
            commands,
        } => {
            let screen = &mut model.config_transfer;
            screen.message = Some(screen.summary());
            screen.preview = None;
            screen.resolutions.clear();
            screen.focus = 0;
            screen.needing_secret = needing_secret;
            screen.commands = commands;
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
    // A table document puts the grid in the editor's slot: `effective_focus` reads pane 2
    // as the grid there, and pane 3 is the grid too, by its name.
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
        // Alt+3 is Focus Results: on a table document too, where the rows are the grid in
        // the editor's slot -- not the log beneath them, which only a click reaches.
        FocusTarget::Results => {
            model.panes.results_visible = true;
            Focus::Results
        }
    };
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
        Some(OverlayKind::RunPrompt) => match hit {
            Some(HitTarget::FormField(_)) => {
                if let Some(prompt) = model.run_prompt.as_mut()
                    && prompt.expected.is_some()
                {
                    prompt.footer = crate::widgets::form::FooterFocus::Input;
                }
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_run_prompt(model),
            Some(HitTarget::FooterCancel) => {
                model.run_prompt = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::ProductionPrompt) => match hit {
            Some(HitTarget::FooterSubmit) => submit_production_prompt(model),
            Some(HitTarget::FooterCancel) => {
                model.production_prompt = None;
                Vec::new()
            }
            Some(HitTarget::FormField(_)) => {
                if let Some(prompt) = model.production_prompt.as_mut() {
                    prompt.footer = crate::widgets::form::FooterFocus::Input;
                }
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::QuitPrompt) => match hit {
            Some(HitTarget::FooterSubmit) => update(model, Action::Quit),
            Some(HitTarget::FooterCancel) => {
                model.quit_prompt = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::ExplainPrompt) => match hit {
            Some(HitTarget::FooterSubmit) => update(model, Action::RunExplainAnalyze),
            Some(HitTarget::FooterCancel) => {
                model.explain_prompt = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
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
        Some(OverlayKind::ObjectOverlay) => mouse_inspector(model, hit),
        Some(OverlayKind::SchemaForm) => match hit {
            Some(HitTarget::FormField(index))
                if index < model.schema_editor.fields.len() && !model.schema_editor.is_raw() =>
            {
                model.schema_editor.focus = index;
                model.schema_editor.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_schema_form(model),
            Some(HitTarget::FooterCancel) => {
                model.schema_editor.open = false;
                Vec::new()
            }
            _ => Vec::new(),
        },
        // A click on the value is a click on text to read; one away from it dismisses.
        Some(OverlayKind::ValueViewer) => {
            if hit != Some(HitTarget::Overlay) {
                model.data.viewer = None;
            }
            Vec::new()
        }
        Some(OverlayKind::Connections) => mouse_connections(model, hit, doubled),
        Some(OverlayKind::Projects) => mouse_projects(model, hit, doubled),
        Some(OverlayKind::ConfigTransfer) => mouse_config_transfer(model, hit),
        Some(OverlayKind::SecretPrompt) => mouse_secret(model, hit),
        Some(OverlayKind::TransactionPrompt) => mouse_transaction(model, hit),
        Some(OverlayKind::DocumentNamePrompt) => mouse_document_name(model, hit),
        Some(OverlayKind::InsertRow) => mouse_insert_row(model, hit),
        Some(OverlayKind::CellEdit) => mouse_cell_edit(model, hit),
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
        Some(OverlayKind::TryIndex) => match hit {
            Some(HitTarget::FormField(0)) => {
                if let Some(prompt) = &mut model.try_index {
                    prompt.footer = crate::widgets::form::FooterFocus::Input;
                }
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_try_index(model),
            Some(HitTarget::FooterCancel) => {
                model.try_index = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::SaveQuery) => match hit {
            Some(HitTarget::FormField(0)) => {
                if let Some(prompt) = &mut model.save_query_prompt {
                    prompt.footer = crate::widgets::form::FooterFocus::Input;
                }
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_save_query(model),
            Some(HitTarget::FooterCancel) => {
                model.save_query_prompt = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::SavedQueries) => match hit {
            // A rename or a delete in progress is answered with its keys, not a click.
            Some(HitTarget::ListRow(index))
                if model.saved_queries.deleting.is_none()
                    && model.saved_queries.renaming.is_none() =>
            {
                model.saved_queries.selected = index;
                open_saved_query(model)
            }
            Some(HitTarget::FooterSubmit) if model.saved_queries.deleting.is_some() => {
                model.saved_queries.deleting = Some(crate::widgets::form::FooterFocus::Submit);
                saved_queries_key(model, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            }
            Some(HitTarget::FooterCancel) => {
                model.saved_queries.deleting = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
        Some(OverlayKind::Related) => match hit {
            Some(HitTarget::ListRow(index)) => {
                if let Some(picker) = &mut model.data.related_picker {
                    picker.selected = index;
                }
                open_related_link(model)
            }
            _ => Vec::new(),
        },
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
    if matches!(hit, Some(HitTarget::Button(HitButton::Close)) | None) {
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
    use crate::screens::secret_prompt::SecretChoiceKind;
    match hit {
        Some(HitTarget::FooterSubmit) => {
            let kind = if model.secret_prompt.keychain {
                SecretChoiceKind::SaveToKeychain
            } else {
                SecretChoiceKind::SessionOnly
            };
            update(model, Action::SubmitSecret { kind })
        }
        Some(HitTarget::Button(HitButton::Keychain)) => {
            model.secret_prompt.keychain =
                !model.secret_prompt.keychain && !model.secret_prompt.temporary;
            model.secret_prompt.keychain_focus = !model.secret_prompt.temporary;
            Vec::new()
        }
        Some(HitTarget::FooterCancel | HitTarget::Overlay) => update(
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

fn mouse_insert_row(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FormField(index)) => {
            model.data.insert_form.focus = index;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => update(model, Action::SubmitInsertRow),
        Some(HitTarget::FooterCancel) => update(model, Action::CancelInsertRow),
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

fn mouse_projects(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    use crate::model::CloseChoice;
    // The question about unsaved documents has three buttons and nothing else to click.
    if model.projects.asking_about_unsaved() {
        return match hit {
            Some(HitTarget::Button(HitButton::Confirm)) => {
                resolve_project_switch(model, CloseChoice::Save)
            }
            Some(HitTarget::Button(HitButton::Discard)) => {
                resolve_project_switch(model, CloseChoice::Discard)
            }
            Some(HitTarget::Button(HitButton::Cancel)) => {
                resolve_project_switch(model, CloseChoice::Cancel)
            }
            _ => Vec::new(),
        };
    }
    let deleting = model.projects.delete.is_some();
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
        Some(HitTarget::FooterSubmit) if deleting => update(model, Action::ConfirmProjectDelete),
        Some(HitTarget::FooterSubmit) => submit_project_name(model),
        Some(HitTarget::FooterCancel) if deleting => {
            model.projects.delete = None;
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterCancel) => {
            model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
            model.projects.name_input.clear();
            model.projects.error = None;
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            if !model.projects.from_list {
                model.projects.open = false;
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::ToggleConnections)) => {
            if let Some(delete) = &mut model.projects.delete {
                delete.delete_connections = !delete.delete_connections;
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_config_transfer(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    // A button of the row shown, by the position it has in it.
    let press = |model: &mut Model, label: &str| {
        let at = model
            .config_transfer
            .buttons()
            .iter()
            .position(|button| *button == label);
        match at {
            Some(index) => {
                model.config_transfer.focus = index;
                press_config_button(model)
            }
            None => Vec::new(),
        }
    };
    match hit {
        // A click picks a clash; a second click on the picked one changes its answer.
        Some(HitTarget::ListRow(index)) => {
            let picked = model.config_transfer.selected == index;
            model.config_transfer.selected = index;
            if picked {
                model.config_transfer.cycle_selected();
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Export)) => press(model, "Export"),
        Some(HitTarget::Button(HitButton::Apply)) => press(model, "Import"),
        Some(HitTarget::Button(HitButton::Close)) => press(model, "Close"),
        Some(HitTarget::Button(HitButton::Confirm)) => press(model, "Overwrite"),
        Some(HitTarget::Button(HitButton::Cancel)) => press(model, "Cancel"),
        _ => Vec::new(),
    }
}

fn mouse_connections(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) => {
            if index < model.connections.row_count() {
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
        Some(HitTarget::Button(HitButton::Connect)) => choose_connection_intent(model),
        Some(HitTarget::Button(HitButton::Docker)) => vec![Effect::DiscoverDocker],
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
        Some(HitTarget::FormChoice { index, step }) => {
            if index < model.connection_form.fields.len() {
                model.connection_form.focus = index;
                model.connection_form.cycle_choice(i32::from(step));
            }
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) => save_connection(model),
        Some(HitTarget::Button(HitButton::Test)) => test_connection(model),
        Some(HitTarget::FooterCancel) => {
            model.connection_form.close();
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_file_picker(model: &mut Model, hit: Option<HitTarget>, doubled: bool) -> Vec<Effect> {
    let rows = file_picker_rows(model);
    if model.file_picker.confirm.is_some() {
        return match hit {
            Some(HitTarget::FooterSubmit) => replace_file(model),
            Some(HitTarget::FooterCancel) => {
                model.file_picker.confirm = None;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
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
        // Only the buttons answer: a click anywhere in the box used to clear everything.
        return match hit {
            Some(HitTarget::FooterSubmit) => confirm_clear_history(model),
            Some(HitTarget::FooterCancel) => {
                model.editor.history_confirm_clear = false;
                model.editor.history_open = false;
                Vec::new()
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
    if let Some(prompt) = model.admin.terminate.as_mut() {
        return match hit {
            Some(HitTarget::FooterSubmit) => submit_terminate(model),
            Some(HitTarget::FooterCancel) => {
                model.admin.terminate = None;
                Vec::new()
            }
            Some(HitTarget::FormField(_)) => {
                prompt.footer = crate::widgets::form::FooterFocus::Input;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if let Some(HitTarget::ListRow(index)) = hit
        && index < model.admin.sessions.len()
    {
        model.admin.selected = index;
    }
    Vec::new()
}

fn mouse_ddl_preview(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::FooterSubmit) => apply_ddl(model),
        Some(HitTarget::FooterCancel) => cancel_ddl_preview(model),
        Some(HitTarget::FormField(0)) => {
            if let Some(preview) = model.schema_editor.preview.as_mut() {
                preview.footer = crate::widgets::form::FooterFocus::Input;
            }
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
        // A click on a source steps to the next one; the keys step either way.
        Some(HitTarget::FormField(side)) if model.schema_diff.source_prompt && side < 2 => {
            model.schema_diff.footer = crate::widgets::form::FooterFocus::Input;
            model.schema_diff.row = side;
            model.schema_diff.cycle(side, 1);
            Vec::new()
        }
        Some(HitTarget::FormField(2)) if model.schema_diff.source_prompt => {
            model.schema_diff.footer = crate::widgets::form::FooterFocus::Input;
            model.schema_diff.row = 2;
            Vec::new()
        }
        Some(HitTarget::ListRow(index)) if !model.schema_diff.source_prompt => {
            model.schema_diff.selected = index;
            model.schema_diff.footer = crate::widgets::form::FooterFocus::Input;
            Vec::new()
        }
        Some(HitTarget::FooterSubmit) if model.schema_diff.source_prompt => {
            crate::screens::schema_diff::request(model)
        }
        Some(HitTarget::FooterSubmit) => crate::screens::schema_diff::open_script(model),
        Some(HitTarget::FooterCancel) => {
            model.schema_diff.open = false;
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
        Some(HitTarget::Button(HitButton::Apply) | HitTarget::FooterSubmit) => {
            update(model, Action::ApplyChanges)
        }
        Some(HitTarget::FooterCancel) => {
            model.data.review = None;
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Discard)) => discard_from_review(model),
        _ => Vec::new(),
    }
}

/// `d` in the review, or its button: every pending change goes, and the review with them.
fn discard_from_review(model: &mut Model) -> Vec<Effect> {
    discard_all_pending(model);
    model.data.review = None;
    model.messages.info("Pending changes discarded.".into());
    Vec::new()
}

/// The review's keys, those of every Submit/Cancel dialog: the arrows and Tab walk Apply
/// and Cancel, Esc and Cancel close it with the changes still pending, Enter takes the
/// button that has the focus. PageUp, PageDown, Home and End scroll the statements, and
/// `d` discards everything.
fn review_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, confirm_key};
    let Some(review) = model.data.review.as_mut() else {
        return Vec::new();
    };
    let page = i32::from((model.height / 2).max(1));
    let delta = match key.code {
        KeyCode::PageUp => Some(-page),
        KeyCode::PageDown => Some(page),
        KeyCode::Home => Some(-i32::from(u16::MAX)),
        KeyCode::End => Some(i32::from(u16::MAX)),
        _ => None,
    };
    if let Some(delta) = delta {
        review.scroll = model
            .hits
            .scroll(crate::mouse::ScrollArea::Review, review.scroll, delta);
        return Vec::new();
    }
    if key.code == KeyCode::Char('d') && key.modifiers.is_empty() {
        return discard_from_review(model);
    }
    match confirm_key(&mut review.footer, &key) {
        FooterKey::Submit => update(model, Action::ApplyChanges),
        FooterKey::Cancel => {
            model.data.review = None;
            Vec::new()
        }
        FooterKey::Moved | FooterKey::Pass => Vec::new(),
    }
}

/// New MCP Grant's keys: Tab and the arrows walk the fields and the buttons, Left and
/// Right step between the buttons, Space flips "ask before each write", Enter creates
/// unless Cancel has the focus, Esc closes.
fn grant_form_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::FooterFocus;
    let Some(form) = model.mcp_profiles.grant_form.as_mut() else {
        return Vec::new();
    };
    let footer = form.footer_focus();
    match key.code {
        KeyCode::Esc => close_grant_form(model),
        KeyCode::Enter if footer == FooterFocus::Cancel => close_grant_form(model),
        KeyCode::Enter => return submit_grant_form(model),
        KeyCode::Down | KeyCode::Tab => form.focus_next(),
        KeyCode::Up | KeyCode::BackTab => form.focus_prev(),
        KeyCode::Left | KeyCode::Right if footer != FooterFocus::Input => form.toggle_button(),
        _ if footer == FooterFocus::Input => form.edit(key),
        _ => {}
    }
    Vec::new()
}

fn submit_grant_form(model: &mut Model) -> Vec<Effect> {
    let Some(form) = model.mcp_profiles.grant_form.as_mut() else {
        return Vec::new();
    };
    let profile = form.profile_name();
    if profile.is_empty() {
        form.error =
            Some("there is no profile to grant to: create one with dexo mcp profile create".into());
        return Vec::new();
    }
    match form.request() {
        Ok(request) => {
            form.error = None;
            vec![Effect::CreateMcpGrant { profile, request }]
        }
        Err(error) => {
            form.error = Some(error);
            Vec::new()
        }
    }
}

fn mouse_mcp_profiles(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    if model.mcp_profiles.grant_form.is_some() {
        return match hit {
            Some(HitTarget::FormField(index)) => {
                if let Some(form) = model.mcp_profiles.grant_form.as_mut() {
                    form.focus = index;
                    // A click on the checkbox flips it, as Space does.
                    if index == crate::screens::mcp_profiles::GRANT_ASK {
                        form.toggle_ask();
                    }
                }
                Vec::new()
            }
            Some(HitTarget::FormChoice { index, step }) => {
                if let Some(form) = model.mcp_profiles.grant_form.as_mut() {
                    form.focus = index;
                    form.step(isize::from(step));
                }
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_grant_form(model),
            Some(HitTarget::FooterCancel) => {
                close_grant_form(model);
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.mcp_profiles.confirm.is_some() {
        return match hit {
            Some(HitTarget::FooterSubmit) => commit_mcp_confirm(model),
            Some(HitTarget::FooterCancel) => {
                model.mcp_profiles.confirm = None;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    match hit {
        Some(HitTarget::ListRow(index)) => {
            model.mcp_profiles.select_index(index);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// The New MCP Grant form, for the profile picked -- or the first, from the palette.
fn open_grant_form(model: &mut Model) {
    let screen = &mut model.mcp_profiles;
    if screen.profiles.is_empty() {
        // A form for nobody cannot succeed; the screen says how to make a profile.
        screen.grant_from_palette = false;
        screen.status = "Create a profile first: dexo mcp profile create --name NAME.".into();
        return;
    }
    let choices = screen
        .profiles
        .iter()
        .map(|profile| crate::screens::mcp_profiles::ProfileChoice {
            name: profile.name.clone(),
            connections: profile.connections.clone(),
        })
        .collect();
    screen.grant_form = Some(crate::screens::mcp_profiles::GrantForm::new(
        choices,
        screen.selected,
    ));
}

/// Cancel or Esc on the grant form: back to the profiles, or out, when the palette asked.
fn close_grant_form(model: &mut Model) {
    model.mcp_profiles.grant_form = None;
    if model.mcp_profiles.grant_from_palette {
        model.mcp_profiles.open = false;
        model.mcp_profiles.grant_from_palette = false;
    }
}

/// What the confirmation asked is done.
fn commit_mcp_confirm(model: &mut Model) -> Vec<Effect> {
    use crate::screens::mcp_profiles::McpConfirmKind;
    let Some(confirm) = model.mcp_profiles.confirm.take() else {
        return Vec::new();
    };
    match confirm.kind {
        McpConfirmKind::Enable(name) => {
            model.mcp_profiles.status =
                format!("Enabled {name}: agents can now use what it allows.");
            vec![Effect::SetMcpProfileEnabled {
                name,
                enabled: true,
            }]
        }
        McpConfirmKind::RevokeProfile { name, .. } => {
            vec![Effect::RevokeMcpGrants { profile: name }]
        }
        McpConfirmKind::RevokeAll { .. } => vec![Effect::RevokeAllMcpGrants],
        McpConfirmKind::DeleteProfile(name) => vec![Effect::DeleteMcpProfile { name }],
    }
}

fn mouse_settings(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        Some(HitTarget::ListRow(index)) if index < crate::screens::settings::FIELD_COUNT => {
            model.settings.focus = index;
            model.settings.confirm_reset = false;
            step_focused_setting(model, 1)
        }
        Some(HitTarget::SettingsChoice { row, index })
            if row < crate::screens::settings::FIELD_COUNT =>
        {
            model.settings.focus = row;
            model.settings.confirm_reset = false;
            // Steps forward to the clicked value, as many times as it takes; the same
            // path the arrows take, so every setting applies and saves as it does there.
            let options = model.settings.options();
            let Some(field) = options.get(row) else {
                return Vec::new();
            };
            let count = field.values.len();
            let steps = (index + count - field.active) % count.max(1);
            let mut effects = Vec::new();
            for _ in 0..steps {
                effects.extend(step_focused_setting(model, 1));
            }
            effects
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

/// A click closes the inspector -- unless a note is being written: then [Save] and
/// [Cancel] answer, and a click anywhere else keeps what was typed.
fn mouse_inspector(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    if model.inspector.editing_note.is_none() {
        model.inspector.open = false;
        return Vec::new();
    }
    match hit {
        Some(HitTarget::FooterSubmit) => submit_note(model),
        Some(HitTarget::FooterCancel) => {
            model.inspector.editing_note = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_mcp_audit(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    match hit {
        // A click on a waiting request picks it.
        Some(HitTarget::ListRow(index)) => {
            if let Some(id) = model
                .mcp_audit
                .pending
                .get(index)
                .map(|request| request.id)
                .filter(|_| model.mcp_audit.deciding.is_none())
            {
                model.mcp_audit.selected = Some(id);
                model.mcp_audit.scroll = 0;
            }
            Vec::new()
        }
        Some(HitTarget::Button(HitButton::Revoke)) => update(model, Action::RevokeAllMcpGrants),
        Some(HitTarget::FooterSubmit) => match model.mcp_audit.deciding.take() {
            Some(deciding) => vec![Effect::SettleApproval {
                id: deciding.id,
                approve: deciding.approve,
            }],
            None => Vec::new(),
        },
        Some(HitTarget::FooterCancel) => {
            model.mcp_audit.deciding = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn mouse_workbench(
    model: &mut Model,
    mouse: MouseEvent,
    hit: Option<HitTarget>,
    doubled: bool,
) -> Vec<Effect> {
    // Most terminals keep Shift+click for their own text selection, so Alt extends too
    // (and a right click on a header adds its column). Ctrl picks rows.
    let extend = mouse
        .modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT);
    let pick = mouse.modifiers.contains(KeyModifiers::CONTROL);
    match hit {
        Some(HitTarget::ResultTab(index)) => update(model, Action::SelectResultTab { index }),
        Some(HitTarget::ResultsView(index)) => {
            if let Some(view) = crate::model::ResultsView::ALL.get(index).copied() {
                model.results.view = view;
                scroll_results_view_to_start(model);
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
        Some(HitTarget::ExplorerTwistie(index)) => {
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
            expand_or_open_selected(model)
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
        Some(HitTarget::ClauseBar(bar)) => update(model, Action::FocusClauseBar { bar }),
        Some(HitTarget::GridHeader(col)) => {
            crate::screens::editor::end_typing(model);
            close_palette(model);
            model.focus = Focus::Results;
            select_header_column(model, col);
            // A grid that cannot run again keeps its rows; the click only selects.
            if clause_bars_shown(model) {
                sort_by_column(model, col, extend)
            } else {
                Vec::new()
            }
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
            if doubled && model.active_document().kind.is_table() && !extend {
                // A table's cell is edited where it is double-clicked, as in any grid;
                // the menu is Enter's, and a right click's.
                update(model, Action::EditCell)
            } else if doubled {
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
        Some(HitTarget::Console) => {
            let effects = update(model, Action::Focus(FocusTarget::Results));
            if model.active_document().kind.is_table() {
                model.focus = Focus::Console;
            }
            effects
        }
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
        // A right click on a header adds its column to the sort, as Shift+click would
        // where the terminal leaves Shift alone.
        Some(HitTarget::GridHeader(col)) => {
            model.focus = Focus::Results;
            select_header_column(model, col);
            if clause_bars_shown(model) {
                sort_by_column(model, col, true)
            } else {
                Vec::new()
            }
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
    model.panes = model.panes.fit(model.width, model.height);
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
            model.help.scroll =
                model
                    .hits
                    .scroll(crate::mouse::ScrollArea::Help, model.help.scroll, -1);
        } else {
            model.help.scroll =
                model
                    .hits
                    .scroll(crate::mouse::ScrollArea::Help, model.help.scroll, 1);
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
        } else {
            model.editor.history_selected += 1;
            model.editor.clamp_history();
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Review) {
        if let Some(review) = model.data.review.as_mut() {
            review.scroll =
                model
                    .hits
                    .scroll(crate::mouse::ScrollArea::Review, review.scroll, delta * 3);
        }
        return Vec::new();
    }
    if overlay == Some(OverlayKind::ValueViewer) {
        model.data.viewer_scroll = model.hits.scroll(
            crate::mouse::ScrollArea::Value,
            model.data.viewer_scroll,
            delta,
        );
        return Vec::new();
    }
    if overlay == Some(OverlayKind::Related) {
        if let Some(picker) = &mut model.data.related_picker {
            let last = picker.links.as_ref().map_or(0, Vec::len).saturating_sub(1);
            picker.selected = if delta < 0 {
                picker.selected.saturating_sub(1)
            } else {
                (picker.selected + 1).min(last)
            };
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
    if overlay == Some(OverlayKind::DdlPreview) {
        if let Some(preview) = model.schema_editor.preview.as_mut() {
            preview.scroll = model.hits.scroll(
                crate::mouse::ScrollArea::DdlPreview,
                preview.scroll.min(usize::from(u16::MAX)) as u16,
                delta,
            ) as usize;
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
    if overlay == Some(OverlayKind::McpAudit) {
        model.mcp_audit.scroll = model.hits.scroll(
            crate::mouse::ScrollArea::McpAudit,
            model.mcp_audit.scroll,
            delta,
        );
        return Vec::new();
    }
    if overlay == Some(OverlayKind::McpProfiles) {
        // The new grant is for the profile picked when it was opened.
        if model.mcp_profiles.grant_form.is_some() {
            return Vec::new();
        }
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
    if overlay == Some(OverlayKind::ConfigTransfer) {
        model.config_transfer.select(delta.signum() as isize);
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
            crate::screens::editor::scroll_view(model, delta.signum());
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
                crate::screens::editor::scroll_view(model, delta.signum());
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
    // Without the kitty keyboard protocol, ^H is what Ctrl+Backspace sends; taking it for
    // Ctrl+H (find and replace) would leave those terminals no key to delete a word.
    let key = if !model.keys_disambiguated
        && key.code == KeyCode::Char('h')
        && key.modifiers == KeyModifiers::CONTROL
    {
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL)
    } else {
        key
    };
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
    if model.run_prompt.is_some() {
        return handle_run_prompt_key(model, key);
    }
    if model.production_prompt.is_some() {
        return handle_production_prompt_key(model, key);
    }
    if model.explain_prompt.is_some() {
        return handle_explain_prompt_key(model, key);
    }
    if model.quit_prompt.is_some() {
        return handle_quit_prompt_key(model, key);
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
    if model.schema_editor.open {
        return schema_form_key(model, key);
    }
    if model.inspector.open {
        use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
        if let Some((input, focus)) = model.inspector.editing_note.as_mut() {
            return match footer_key(focus, &key) {
                FooterKey::Submit => submit_note(model),
                FooterKey::Cancel => {
                    model.inspector.editing_note = None;
                    Vec::new()
                }
                FooterKey::Moved => Vec::new(),
                FooterKey::Pass => {
                    if *focus == FooterFocus::Input {
                        input.handle_key(key);
                    }
                    Vec::new()
                }
            };
        }
        return match key.code {
            KeyCode::Char('n') if model.inspector.object.is_some() => {
                start_note_editor(model);
                Vec::new()
            }
            KeyCode::Esc => {
                model.inspector.open = false;
                Vec::new()
            }
            KeyCode::Up => {
                model.inspector.scroll = model.hits.scroll(
                    crate::mouse::ScrollArea::Inspector,
                    model.inspector.scroll,
                    -1,
                );
                Vec::new()
            }
            KeyCode::Down => {
                model.inspector.scroll = model.hits.scroll(
                    crate::mouse::ScrollArea::Inspector,
                    model.inspector.scroll,
                    1,
                );
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.data.viewer.is_some() {
        let page = i32::from((model.height / 2).max(1));
        let delta = match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                model.data.viewer = None;
                return Vec::new();
            }
            KeyCode::Up => -1,
            KeyCode::Down => 1,
            KeyCode::PageUp => -page,
            KeyCode::PageDown => page,
            KeyCode::Home => -i32::from(u16::MAX),
            KeyCode::End => i32::from(u16::MAX),
            _ => 0,
        };
        model.data.viewer_scroll = model.hits.scroll(
            crate::mouse::ScrollArea::Value,
            model.data.viewer_scroll,
            delta,
        );
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
    if model.save_query_prompt.is_some() {
        return save_query_key(model, key);
    }
    if model.try_index.is_some() {
        return try_index_key(model, key);
    }
    // In the Explain view `i` tries an index; elsewhere in the results it inserts a row.
    if model.effective_focus() == Focus::Results
        && crate::mouse::top_overlay(model).is_none()
        && model.results.view == crate::model::ResultsView::Explain
        && key.code == KeyCode::Char('i')
        && key.modifiers.is_empty()
    {
        return update(model, Action::OpenTryIndex);
    }
    if model.saved_queries.open {
        return saved_queries_key(model, key);
    }
    if let Some(picker) = &mut model.data.related_picker {
        let count = picker.links.as_ref().map_or(0, Vec::len);
        match key.code {
            KeyCode::Esc => model.data.related_picker = None,
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down if picker.selected + 1 < count => picker.selected += 1,
            KeyCode::Enter => return open_related_link(model),
            _ => {}
        }
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
        return ddl_preview_key(model, key);
    }
    if model.schema_diff.open {
        return crate::screens::schema_diff::handle_key(model, key);
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
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                open_file_picker(model, crate::screens::file_picker::FilePickerMode::Transfer);
                Vec::new()
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                model.transfer.format = next_transfer_format(
                    &model.transfer.format,
                    model.transfer.mode == crate::screens::transfer::TransferMode::Import,
                );
                Vec::new()
            }
            _ if model.transfer.footer == crate::widgets::form::FooterFocus::Input => {
                model.transfer.path.handle_key(key);
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if model.data.insert_form.open {
        use crate::widgets::form::FooterFocus;
        let footer = model.data.insert_form.footer_focus();
        let form = &mut model.data.insert_form;
        match key.code {
            KeyCode::Esc => return update(model, Action::CancelInsertRow),
            KeyCode::Enter if footer == FooterFocus::Cancel => {
                return update(model, Action::CancelInsertRow);
            }
            KeyCode::Enter => return update(model, Action::SubmitInsertRow),
            KeyCode::Down | KeyCode::Tab => form.focus_next(),
            KeyCode::Up | KeyCode::BackTab => form.focus_prev(),
            KeyCode::Left | KeyCode::Right if footer != FooterFocus::Input => form.toggle_button(),
            _ => {
                if let Some(field) = form.focused_field_mut() {
                    field.value.handle_key(key);
                }
            }
        }
        return Vec::new();
    }
    if model.data.cell_edit.is_some() {
        return cell_edit_key(model, key);
    }
    if model.data.review.is_some() {
        return review_key(model, key);
    }
    if model.mcp_profiles.open {
        if model.mcp_profiles.grant_form.is_some() {
            return grant_form_key(model, key);
        }
        // A confirmation takes the keys: Enter on the focused button, Esc cancels, and no
        // letter confirms, so typing at the screen cannot enable or revoke anything.
        if let Some(confirm) = model.mcp_profiles.confirm.as_mut() {
            return match crate::widgets::form::confirm_key(&mut confirm.focus, &key) {
                crate::widgets::form::FooterKey::Submit => commit_mcp_confirm(model),
                crate::widgets::form::FooterKey::Cancel => {
                    model.mcp_profiles.confirm = None;
                    Vec::new()
                }
                _ => Vec::new(),
            };
        }
        // Ctrl and Alt chords are not this screen's letters.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Vec::new();
        }
        let page = (model.height / 3).max(1) as isize;
        return match key.code {
            KeyCode::Esc => {
                model.mcp_profiles.open = false;
                model.mcp_profiles.confirm = None;
                Vec::new()
            }
            KeyCode::Char('g') => update(model, Action::OpenMcpGrantForm),
            KeyCode::Up => {
                model.mcp_profiles.select_previous();
                Vec::new()
            }
            KeyCode::Down => {
                model.mcp_profiles.select_next();
                Vec::new()
            }
            KeyCode::Home => {
                model.mcp_profiles.select_index(0);
                Vec::new()
            }
            KeyCode::End => {
                model.mcp_profiles.select_index(usize::MAX);
                Vec::new()
            }
            KeyCode::PageDown => {
                let screen = &mut model.mcp_profiles;
                screen.detail_scroll = screen.detail_scroll.saturating_add(page as usize);
                Vec::new()
            }
            KeyCode::PageUp => {
                let screen = &mut model.mcp_profiles;
                screen.detail_scroll = screen.detail_scroll.saturating_sub(page as usize);
                Vec::new()
            }
            KeyCode::Char('e') => update(model, Action::ToggleMcpProfile),
            KeyCode::Char('r') => update(model, Action::RevokeProfileGrants),
            KeyCode::Char('R') => {
                model.mcp_profiles.ask_revoke_all();
                Vec::new()
            }
            KeyCode::Char('x') => {
                model.mcp_profiles.ask_delete();
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
            KeyCode::Char('e') => update(model, Action::CycleTheme),
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
        use crate::widgets::form::{FooterFocus, FooterKey};
        // A statement taller than the popup is read page by page, the confirmation open
        // or not.
        let page = i32::from((model.height / 3).max(1));
        let paged = match key.code {
            KeyCode::PageDown => Some(page),
            KeyCode::PageUp => Some(-page),
            _ => None,
        };
        if let Some(delta) = paged {
            model.mcp_audit.scroll = model.hits.scroll(
                crate::mouse::ScrollArea::McpAudit,
                model.mcp_audit.scroll,
                delta,
            );
            return Vec::new();
        }
        let screen = &mut model.mcp_audit;
        if let Some(deciding) = screen.deciding.as_mut() {
            return match crate::widgets::form::confirm_key(&mut deciding.focus, &key) {
                FooterKey::Submit => {
                    let (id, approve) = (deciding.id, deciding.approve);
                    screen.deciding = None;
                    vec![Effect::SettleApproval { id, approve }]
                }
                FooterKey::Cancel => {
                    screen.deciding = None;
                    Vec::new()
                }
                FooterKey::Moved | FooterKey::Pass => Vec::new(),
            };
        }
        return match key.code {
            KeyCode::Esc => {
                screen.open = false;
                Vec::new()
            }
            // With nothing waiting, the arrows read the recent calls instead.
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
                if screen.pending.is_empty() =>
            {
                let delta = match key.code {
                    KeyCode::Up => -1,
                    KeyCode::Down => 1,
                    KeyCode::Home => -100_000,
                    _ => 100_000,
                };
                screen.scroll =
                    model
                        .hits
                        .scroll(crate::mouse::ScrollArea::McpAudit, screen.scroll, delta);
                Vec::new()
            }
            KeyCode::Up => {
                screen.select(-1);
                Vec::new()
            }
            KeyCode::Down => {
                screen.select(1);
                Vec::new()
            }
            // Approving runs a write: Cancel holds the focus, so an Enter out of habit
            // decides nothing. Denying is safe, and is one Enter away.
            KeyCode::Char(answer @ ('a' | 'd')) => {
                if let Some(request) = screen.current() {
                    let approve = answer == 'a';
                    screen.deciding = Some(crate::screens::mcp_audit::Deciding {
                        id: request.id,
                        approve,
                        focus: if approve {
                            FooterFocus::Cancel
                        } else {
                            FooterFocus::Submit
                        },
                    });
                }
                Vec::new()
            }
            KeyCode::Char('R' | 'r') => update(model, Action::RevokeAllMcpGrants),
            _ => Vec::new(),
        };
    }
    if let Some(effects) = clause_bar_key(model, key) {
        return effects;
    }
    if model.find.open
        && model.effective_focus() == Focus::Editor
        && model.pending_chord.keys.is_empty()
        && crate::screens::find::handle_key(model, key)
    {
        return Vec::new();
    }
    // Vim's `:` and `/` lines take the keys an input edits with before the keymap: Ctrl+W
    // deletes a word there, as in Vim, where it used to close the document.
    if crate::screens::vim::prompt_open(model)
        && model.effective_focus() == Focus::Editor
        && crate::widgets::text_input::TextInput::owns(&key)
    {
        let _ = crate::screens::vim::handle_key(model, key);
        return Vec::new();
    }
    // A terminal sends Alt+key as Esc and the key, so Esc then `o` typed fast in Insert
    // mode arrives as Alt+O and opened the saved queries. Vim reads it as the two keys
    // wherever Esc changes the mode, and so does Dexo.
    if crate::screens::vim::active(model)
        && model.effective_focus() == Focus::Editor
        && matches!(
            crate::screens::vim::mode(model),
            crate::screens::vim::Mode::Insert
                | crate::screens::vim::Mode::Visual
                | crate::screens::vim::Mode::VisualLine
        )
        && key.modifiers.difference(KeyModifiers::SHIFT) == KeyModifiers::ALT
        && let KeyCode::Char(ch) = key.code
    {
        let mut effects = update(
            model,
            Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        effects.extend(update(
            model,
            Action::Key(KeyEvent::new(
                KeyCode::Char(ch),
                key.modifiers.difference(KeyModifiers::ALT),
            )),
        ));
        return effects;
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
            // Ctrl+S is the grid's review and the document's save. With nothing pending
            // in a SQL document's grid it is the save: it opened a review of no changes. A
            // table has no file to save, so there the review says nothing is pending.
            let command = if command == "data.review"
                && model.data.changes.pending().is_empty()
                && !model.active_document().kind.is_table()
            {
                "document.save"
            } else {
                command
            };
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
    // Vim mode has the keys the keymap left, before the plain editor does.
    // On "nothing open" too: `i` and the typing after it make the document there.
    if crate::screens::vim::active(model)
        && model.effective_focus() == Focus::Editor
        && !model.active_document().kind.is_table()
        && !model.find.open
    {
        match crate::screens::vim::handle_key(model, key) {
            crate::screens::vim::Outcome::Pass => {}
            crate::screens::vim::Outcome::Done => {
                if model.active_document().sql.revision() != revision {
                    crate::screens::editor::refresh_intelligence(model, false);
                }
                return crate::screens::editor::take_completion_effects(model);
            }
            crate::screens::vim::Outcome::Then(actions) => {
                let mut effects = Vec::new();
                for action in actions {
                    effects.extend(update(model, action));
                }
                return effects;
            }
        }
    }
    // With the find bar open, the keys it does not take still reach the keymap above,
    // never the text underneath.
    if !model.active_document().kind.is_table()
        && !model.find.open
        && crate::screens::editor::handle_key(model, key)
    {
        crate::screens::vim::after_pass(model);
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

pub(crate) fn active_key_context(model: &Model) -> crate::keymap::KeyContext {
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
        // A page is what the list shows; the list has a hundred and forty rows, and the
        // arrows were the only way down it.
        KeyCode::PageUp | KeyCode::PageDown => {
            let count = palette_match_count(model);
            let page = crate::palette::popup_list_rows(model.height, count) as isize;
            move_palette_selection(
                model,
                if key.code == KeyCode::PageUp {
                    -page
                } else {
                    page
                },
            );
            Vec::new()
        }
        // The query edits like any input: Ctrl+A selects it, the word keys work, and
        // a letter typed with Ctrl is never text.
        _ => {
            let before = model.palette.query.as_str().to_string();
            model.palette.query.handle_key(key);
            if model.palette.query.as_str() != before {
                model.palette.selected = 0;
                model.palette.offset = 0;
            }
            Vec::new()
        }
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
        KeyCode::Enter if model.connection_form.on_test() => test_connection(model),
        KeyCode::Enter => save_connection(model),
        KeyCode::Tab | KeyCode::Down => {
            model.connection_form.focus_next();
            Vec::new()
        }
        KeyCode::BackTab | KeyCode::Up => {
            model.connection_form.focus_prev();
            Vec::new()
        }
        KeyCode::Left if model.connection_form.on_cancel() || model.connection_form.on_test() => {
            model.connection_form.focus_prev();
            Vec::new()
        }
        KeyCode::Right if model.connection_form.on_submit() || model.connection_form.on_test() => {
            model.connection_form.focus_next();
            Vec::new()
        }
        KeyCode::Left if model.connection_form.on_choice() => {
            model.connection_form.cycle_choice(-1);
            Vec::new()
        }
        KeyCode::Right if model.connection_form.on_choice() => {
            model.connection_form.cycle_choice(1);
            Vec::new()
        }
        // A text field edits like any input: it used to append and delete from its end
        // only, and the arrows did nothing in it.
        _ => {
            model.connection_form.edit(key);
            Vec::new()
        }
    }
}

/// The secret prompt: type the secret, Alt+K to keep it in the keychain, Enter to use
/// it; the arrows walk Submit and Cancel and Esc cancels, as in every dialog.
fn handle_secret_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::secret_prompt::SecretChoiceKind;
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    let prompt = &mut model.secret_prompt;
    if key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::ALT) {
        prompt.keychain = !prompt.keychain && !prompt.temporary;
        return Vec::new();
    }
    // The keychain checkbox is a stop between the secret and the buttons, so the arrows
    // reach it, and Space flips it there.
    if !prompt.temporary {
        match (prompt.keychain_focus, prompt.footer, key.code) {
            (false, FooterFocus::Input, KeyCode::Tab | KeyCode::Down) => {
                prompt.keychain_focus = true;
                return Vec::new();
            }
            (true, _, KeyCode::Tab | KeyCode::Down) => {
                prompt.keychain_focus = false;
                prompt.footer = FooterFocus::Submit;
                return Vec::new();
            }
            (true, _, KeyCode::BackTab | KeyCode::Up) => {
                prompt.keychain_focus = false;
                prompt.footer = FooterFocus::Input;
                return Vec::new();
            }
            (false, FooterFocus::Submit, KeyCode::BackTab | KeyCode::Up) => {
                prompt.keychain_focus = true;
                prompt.footer = FooterFocus::Input;
                return Vec::new();
            }
            (true, _, KeyCode::Char(' ')) => {
                prompt.keychain = !prompt.keychain;
                return Vec::new();
            }
            _ => {}
        }
    }
    let kind = match footer_key(&mut prompt.footer, &key) {
        FooterKey::Cancel => SecretChoiceKind::Cancel,
        FooterKey::Submit if prompt.keychain => SecretChoiceKind::SaveToKeychain,
        FooterKey::Submit => SecretChoiceKind::SessionOnly,
        FooterKey::Moved => return Vec::new(),
        FooterKey::Pass => {
            if prompt.footer == FooterFocus::Input && !prompt.keychain_focus {
                prompt.buffer.handle_key(key);
                prompt.error = None;
            }
            return Vec::new();
        }
    };
    update(model, Action::SubmitSecret { kind })
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
            if model.connections.selected_profile + 1 < model.connections.row_count() {
                model.connections.selected_profile += 1;
            }
            Vec::new()
        }
        KeyCode::Char('r') => vec![Effect::DiscoverDocker],
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
    use crate::screens::secret_prompt::SecretChoiceKind;
    let profile = model.secret_prompt.profile.clone();
    let purpose = model.secret_prompt.purpose;
    let secret = model.secret_prompt.buffer.clone();
    // A temporary connection's secret_ref names no saved profile; a keychain entry under
    // it would outlive the session with nothing to find or delete it.
    let kind = match kind {
        SecretChoiceKind::SaveToKeychain if model.secret_prompt.temporary => {
            SecretChoiceKind::SessionOnly
        }
        kind => kind,
    };
    model.secret_prompt.close();
    match (kind, profile) {
        (crate::screens::secret_prompt::SecretChoiceKind::Cancel, _) => Vec::new(),
        (_, None) => Vec::new(),
        (kind, Some(profile)) => vec![Effect::SubmitSecret {
            kind,
            purpose,
            profile,
            secret,
            token: model.connections.pending_connect.unwrap_or(0),
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

/// The active connection's driver, and with it the dialect Copy as SQL and the grid's
/// own statements quote in -- set together, so switching sessions cannot leave the one
/// a previous connection spoke.
fn set_connection_driver(model: &mut Model, driver: String) {
    model.data.dialect = dexo_app::dialect_for_driver(&driver).into();
    model.connection.driver = driver;
}

/// Sessions are found by the connection's name: when it changes, its open sessions go
/// with it -- here, in the runtime, and on the status line -- instead of being left
/// under a name nothing has any more.
fn rename_sessions(model: &mut Model, from: &str, to: &str) -> Vec<Effect> {
    let mut renamed = false;
    for row in &mut model.connections.sessions {
        if row.connection == from {
            row.connection = to.to_string();
            renamed = true;
        }
    }
    if model.connection.name == from {
        model.connection.name = to.to_string();
    }
    if renamed {
        vec![Effect::RenameSessions {
            from: from.to_string(),
            to: to.to_string(),
        }]
    } else {
        Vec::new()
    }
}

/// `dexo <url>`: listed beside the saved connections, selected and dialled. A saved
/// connection already going by the same name keeps it; sessions are found by name.
/// `dexo <url>`'s connection, and what its URL warned of: said now, and again once the
/// connection is ready -- under the name it was opened as, which is `name (2)` when a
/// saved connection has the name.
pub(crate) fn open_startup_connection(
    model: &mut Model,
    profile: dexo_app::ConnectionProfile,
    warning: Option<String>,
) -> Vec<Effect> {
    let id = profile.id;
    let effects = open_temporary_connection(model, profile);
    if let Some(warning) = warning {
        model.messages.warn(warning.clone());
        if let Some(opened) = model
            .connections
            .temporary
            .iter()
            .find(|temporary| temporary.id == id)
        {
            model.startup_warning = Some((opened.name.clone(), warning));
        }
    }
    effects
}

fn open_temporary_connection(
    model: &mut Model,
    mut profile: dexo_app::ConnectionProfile,
) -> Vec<Effect> {
    let taken = |name: &str| {
        model
            .connections
            .profiles
            .iter()
            .any(|row| row.profile.name == name)
    };
    if taken(&profile.name) {
        let base = profile.name.clone();
        profile.name = (2..)
            .map(|n| format!("{base} ({n})"))
            .find(|name| !taken(name))
            .expect("an unused name");
    }
    model.connections.temporary.push(profile.clone());
    let saved = model
        .connections
        .profiles
        .iter()
        .filter(|row| !row.temporary)
        .map(|row| row.profile.clone())
        .collect();
    model.connections.load_profiles(saved);
    sync_explorer_connections(model);
    if let Some(index) = model
        .connections
        .profiles
        .iter()
        .position(|row| row.profile.id == profile.id)
    {
        model.connections.selected_profile = index;
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
    set_connection_driver(model, session.driver.clone());
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
        let Some((input, password)) = model.connection_form.submit() else {
            return Vec::new();
        };
        model
            .connection_form
            .set_notice("Testing the connection...".into());
        // An edit that types no new password is tested with the one already saved.
        if let Some(original) = model.connection_form.editing.clone()
            && password.is_empty()
        {
            return match dexo_app::test_connection_input(input) {
                Ok(mut profile) => {
                    profile.secret_ref = original.secret_ref;
                    profile.secret_refs = original.secret_refs;
                    vec![Effect::TestSavedProfile { profile }]
                }
                Err(error) => {
                    model.connection_form.set_error(error.to_string());
                    Vec::new()
                }
            };
        }
        return vec![Effect::TestConnection { input, password }];
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
            // Sessions are found by name: a connection called what an open temporary
            // one is called -- added or renamed -- would take its session over.
            let saving = model.connection_form.saving_temporary.as_ref();
            let clashes = model.connections.temporary.iter().any(|temporary| {
                temporary.name == input.name.trim()
                    && saving.is_none_or(|saving| saving.id != temporary.id)
            });
            if clashes {
                model.connection_form.set_error(format!(
                    "{} is the name of an open temporary connection; pick another",
                    input.name.trim()
                ));
                return Vec::new();
            }
            if let Some(original) = model.connection_form.editing.clone() {
                match dexo_app::test_connection_input(input) {
                    Ok(mut profile) => {
                        profile.id = original.id;
                        profile.secret_ref = original.secret_ref;
                        profile.secret_refs = original.secret_refs;
                        profile.project_id = original.project_id;
                        model.connection_form.close();
                        vec![Effect::SaveProfile { profile, password }]
                    }
                    Err(error) => {
                        model.connection_form.set_error(error.to_string());
                        Vec::new()
                    }
                }
            } else {
                vec![Effect::CreateConnection {
                    input,
                    password,
                    connect: saving.is_none(),
                }]
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
    if model.transaction_prompt.footer == crate::widgets::form::FooterFocus::Input {
        model.transaction_prompt.name.handle_key(key);
    }
    Vec::new()
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

fn palette_match_count(model: &Model) -> usize {
    crate::palette::filter_entries(
        &crate::palette::palette_entries(model),
        model.palette.query.as_str(),
    )
    .len()
}

fn move_palette_selection(model: &mut Model, delta: isize) {
    let count = palette_match_count(model);
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
    focus_explorer_when_nothing_is_open(model);
    vec![Effect::CompleteOnboarding]
}

/// With no document open there is no editor to type into, and the Welcome sends the
/// first key -- `n` -- to the explorer: it started a document with an `n` in it instead.
fn focus_explorer_when_nothing_is_open(model: &mut Model) {
    if model
        .documents
        .iter()
        .all(|document| document.kind.is_placeholder())
    {
        model.panes.explorer_visible = true;
        model.focus = Focus::Explorer;
    }
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
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
            // A page is what the list shows under the search, less a line kept for
            // context; PageUp and PageDown used to move one line, like the arrows.
            let page = i32::from(model.height.saturating_sub(7)).max(1);
            let delta = match key.code {
                KeyCode::Up => -1,
                KeyCode::Down => 1,
                KeyCode::PageUp => -page,
                _ => page,
            };
            model.help.scroll =
                model
                    .hits
                    .scroll(crate::mouse::ScrollArea::Help, model.help.scroll, delta);
            Vec::new()
        }
        KeyCode::Home => {
            model.help.scroll = 0;
            Vec::new()
        }
        KeyCode::End => {
            model.help.scroll =
                model
                    .hits
                    .scroll(crate::mouse::ScrollArea::Help, 0, i32::from(u16::MAX));
            Vec::new()
        }
        // Home and End page the list; the search takes the other keys an input edits
        // with. It took any letter, Ctrl's included: Ctrl+A typed an `a`.
        _ => {
            let before = model.help.query.as_str().to_string();
            model.help.query.handle_key(key);
            if model.help.query.as_str() != before {
                model.help.scroll = 0;
            }
            Vec::new()
        }
    }
}

/// Scrolls the plan or the log by `delta` lines, to its start or end for a very large one;
/// the end of the log leaves its newest entry at the bottom of the pane.
fn scroll_output_view(model: &mut Model, delta: i32) {
    match model.results.view {
        crate::model::ResultsView::Explain => {
            model.results.explain_scroll = model.hits.scroll(
                crate::mouse::ScrollArea::Explain,
                model.results.explain_scroll,
                delta,
            );
        }
        crate::model::ResultsView::Messages => {
            let rows = crate::widgets::grid::record_rows(model);
            let last = model.messages.line_count().saturating_sub(rows) as i32;
            let next = i32::from(model.results.messages_scroll).saturating_add(delta);
            model.results.messages_scroll = next.clamp(0, last.max(0)) as u16;
        }
        crate::model::ResultsView::Grid => {}
    }
}

/// A view just switched to starts where it is read from: a plan at its top, the log at its
/// end -- the entries that matter are the newest, and it opened on the oldest, off screen
/// the others.
fn scroll_results_view_to_start(model: &mut Model) {
    model.results.explain_scroll = 0;
    model.results.messages_scroll = if model.results.view == crate::model::ResultsView::Messages {
        let rows = crate::widgets::grid::record_rows(model);
        u16::try_from(model.messages.line_count().saturating_sub(rows)).unwrap_or(u16::MAX)
    } else {
        0
    };
}

/// Whether the pane shows the record view: one field per line, a field cursor, the
/// records turned with Left and Right.
fn record_view_shown(model: &Model) -> bool {
    model.results.view == crate::model::ResultsView::Grid
        && model.expanded_records
        && model.results.row_count() > 0
}

fn record_capacity(model: &Model) -> usize {
    crate::widgets::grid::record_rows(model).max(1)
}

/// Moves the field cursor `delta` fields, on into the next record past its last field and
/// back into the previous before its first.
fn record_move_field(model: &mut Model, delta: i32) {
    model.results.ensure_cursor();
    let Some((row, col)) = model.results.selection() else {
        return;
    };
    let fields = model.results.columns().len();
    if fields == 0 {
        return;
    }
    let next = col as i64 + i64::from(delta);
    let (to_row, to_col) = if next < 0 {
        match row {
            0 => (0, 0),
            _ => (row - 1, fields - 1),
        }
    } else if next >= fields as i64 {
        if row + 1 >= model.results.row_count() {
            (row, fields - 1)
        } else {
            (row + 1, 0)
        }
    } else {
        (row, next as usize)
    };
    if to_row != row {
        model.results.record_scroll = 0;
    }
    model.results.select_cell(to_row, to_col);
    // The row viewport follows too, for when the grid comes back.
    model.results.move_cursor_row(0, false);
    record_follow_field(model);
}

/// Scrolls the record so the field under the cursor is on screen: the rule is line 0,
/// field `n` is line `n + 1`.
fn record_follow_field(model: &mut Model) {
    let Some((_, col)) = model.results.selection() else {
        return;
    };
    let capacity = record_capacity(model);
    let line = col + 1;
    let scroll = &mut model.results.record_scroll;
    if col == 0 {
        *scroll = 0;
    } else if line < *scroll {
        *scroll = line;
    } else if line >= *scroll + capacity {
        *scroll = line + 1 - capacity;
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
    let (id, invocation) = (entry.id, entry.invocation.clone());
    // These act on a live session, and the menu is titled with the connection it was
    // opened for: on any other session they would list another server's sessions and
    // grants under that name.
    const NEEDS_THE_CONNECTION: [&str; 5] = [
        "schema.security",
        "admin.sessions",
        "backup.dump",
        "backup.restore",
        "explorer.refresh",
    ];
    if NEEDS_THE_CONNECTION.contains(&id)
        && node_menu_kind(model) == Some(crate::palette::NodeMenuKind::Connection)
        && let Some(index) = selected_connection_profile_index(model)
    {
        let profile = model.connections.profiles[index].profile.clone();
        if model.connection.name != profile.name || model.active_session.is_none() {
            return match model.connections.session_for(&profile.name).cloned() {
                Some(session) => {
                    let mut effects = activate_existing_session(model, &profile, session);
                    effects.extend(invoke_palette(model, invocation));
                    effects
                }
                None => {
                    let effects = connect_to(model, profile);
                    model.pending_menu = Some((model.connect_token, invocation));
                    effects
                }
            };
        }
    }
    invoke_palette(model, invocation)
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
    match crate::palette::invocation_by_id(model, id) {
        Some(invocation) => invoke_palette(model, invocation),
        None => Vec::new(),
    }
}

/// A click on a header puts the cursor in that column, on the row it was on: it sorts,
/// it does not select the column for the next copy.
fn select_header_column(model: &mut Model, col: usize) {
    let row = model.results.cursor_row().unwrap_or(0);
    model.results.select_cell(row, col);
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
    // Said, so F10 shows which of the layouts it is on and when the cycle wraps.
    model.messages.info(format!(
        "Layout {} of 4: {}",
        preset.position() + 1,
        preset.label()
    ));
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
    // Dragged or keyed, the results pane keeps room for a row (see `adjust_results_height`).
    let value = if model.active_document().kind.is_table() {
        value
    } else {
        value.max(6)
    };
    if model.active_document().kind.is_table() {
        model.panes.console_height = value;
    } else {
        model.panes.results_height = value;
    }
}

fn adjust_results_height(model: &mut Model, delta: i16) {
    // The results pane keeps room for its border, toolbar, bars, header and a row: below
    // that the grid had no rows, and its bars were drawn over the border. The console of
    // a table document is a log and may be as short as three lines.
    let floor: i16 = if model.active_document().kind.is_table() {
        3
    } else {
        6
    };
    let current = bottom_pane_height(model) as i16;
    if delta < 0 && current <= floor {
        return;
    }
    let next = (current + delta).max(floor) as u16;
    model.panes.results_visible = true;
    set_bottom_pane_height(model, next);
    model.panes = model.panes.fit(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

fn adjust_explorer_width(model: &mut Model, delta: i16) {
    let next = (model.panes.explorer_width as i16 + delta).max(8) as u16;
    model.panes.explorer_visible = true;
    model.panes.explorer_width = next;
    model.panes = model.panes.fit(model.width, model.height);
    model.sync_grid_viewport();
    model.layout_dirty = true;
}

/// Whether `id` is a saved connection's: a temporary one's (`dexo <url>`) is in no
/// table, so nothing stored -- a saved query, a note -- can belong to it.
fn is_saved_connection(model: &Model, id: &str) -> bool {
    model
        .connections
        .profiles
        .iter()
        .any(|row| !row.temporary && row.profile.id.0.to_string() == id)
}

/// The connection a saved query of the active document belongs to: the document's, or
/// the live one for a document bound to none. The picker marks queries against it.
pub(crate) fn query_connection(model: &Model) -> Option<String> {
    model
        .active_document()
        .connection_id
        .clone()
        .or_else(|| active_connection_uuid(model))
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
        // Its connection is gone: deleted, or a temporary one from an earlier run. From
        // now on it is unbound -- wherever you are, which the tab then says.
        let active = active_connection_uuid(model);
        if let Some(document) = model.documents.get_mut(index) {
            document.connection_id = active;
        }
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
    // One run at a time: a second one was queued without a word and started when the
    // first ended, minutes later, with nothing on screen to say it was waiting.
    if matches!(
        action,
        Action::ExecuteStatement | Action::ExecuteSelection | Action::ExecuteDocument
    ) && model.active_query.is_some()
    {
        let cancel = crate::palette::shortcut_for(model, "query.cancel", Some("Ctrl+F2"))
            .unwrap_or_else(|| "Cancel Query".into());
        model.messages.warn(format!(
            "A query is already running. Wait for it, or cancel it with {cancel}."
        ));
        return effects;
    }
    match action {
        Action::ExecuteStatement => {
            if model.editor_selection().is_some() {
                crate::screens::workbench::execute_selection(model);
            } else {
                crate::screens::workbench::execute_current_statement(model);
            }
        }
        Action::ExecuteSelection => {
            if model.editor_selection().is_none() {
                model.messages.warn(
                    "Select some SQL first: Execute Selection runs only the selection.".into(),
                );
                return effects;
            }
            crate::screens::workbench::execute_selection(model)
        }
        Action::ExecuteDocument => crate::screens::workbench::execute_document(model),
        Action::RefreshTableData => {
            effects.extend(refresh_table_data(model));
            return effects;
        }
        Action::OpenDdlPreview => {
            effects.extend(open_ddl_preview(model));
            return effects;
        }
        Action::OpenExplain | Action::RunExplainAnalyze => {
            effects.extend(explain_effect(
                model,
                matches!(action, Action::RunExplainAnalyze),
                None,
                Vec::new(),
            ));
            return effects;
        }
        Action::TryIndex { definition } => {
            // The index is tried on the statement the plan on screen is for, wherever
            // the cursor went since: on another statement the comparison with the plan
            // without the index silently went away.
            let statement = model.results.explain.sql.clone();
            effects.extend(explain_effect(
                model,
                false,
                Some(statement),
                vec![definition],
            ));
            return effects;
        }
        _ => return effects,
    }
    effects.extend(start_query(model));
    effects
}

fn start_query(model: &mut Model) -> Vec<Effect> {
    crate::screens::editor::end_typing(model);
    // A run takes the keys from here on: a completion list left open sat over the
    // confirmation it might bring up, and over the results.
    crate::screens::editor::close_completion(model);
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
        crate::screens::editor::begin_parameter_prompt(model);
        return Vec::new();
    }
    if !model.editor.parameters.is_empty() {
        // Values given before are used again without asking: say which, and how to
        // change them. A secret's value is not repeated.
        let used = model
            .editor
            .parameters
            .iter()
            .map(|parameter| match (&parameter.value, parameter.sensitive) {
                (_, true) => format!("{} = ****", parameter.name),
                (DbValue::Text(text), false) => format!("{} = {text}", parameter.name),
                _ => parameter.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        model
            .messages
            .info(format!("Using {used}; Edit Parameters changes them."));
    }
    let statements = crate::screens::workbench::planned_statements(model);
    if statements.is_empty() {
        return Vec::new();
    }
    // Statement, selection, document and history runs all pass here, so this is the
    // one place the connection's policy is held to.
    let dialect = crate::screens::editor::editor_dialect(model);
    match dexo_app::run_guard::judge(&statements, dialect, &run_policy(model)) {
        dexo_app::run_guard::RunVerdict::Run => launch_script(model, statements),
        dexo_app::run_guard::RunVerdict::Refuse { index, sql } => {
            let first = sql.trim().lines().next().unwrap_or_default().to_string();
            model.messages.error(format!(
                "Not run: {} is read-only, and statement {} is not a read: {first}",
                model.connection.name,
                index + 1,
            ));
            Vec::new()
        }
        dexo_app::run_guard::RunVerdict::Confirm { flagged, typed } => {
            let mut prompt = crate::screens::run_prompt::RunPrompt::new(statements, flagged, typed);
            prompt.connection = model.connection.name.clone();
            prompt.session = model.active_session;
            model.run_prompt = Some(prompt);
            Vec::new()
        }
    }
}

/// The connection's policy as the guard needs it. A profile that cannot be found or
/// resolved -- a temporary connection, a custom label without its policy -- fails
/// closed: destructive statements are confirmed.
fn run_policy(model: &Model) -> dexo_app::run_guard::RunPolicy {
    let confirm_destructive = model
        .connections
        .profiles
        .iter()
        .map(|row| &row.profile)
        .find(|profile| profile.name == model.connection.name)
        .and_then(|profile| {
            dexo_app::ConnectionPolicy::resolve(&profile.environment, &profile.policy).ok()
        })
        .is_none_or(|policy| policy.confirm_destructive);
    dexo_app::run_guard::RunPolicy {
        connection: model.connection.name.clone(),
        read_only: model.connection.read_only,
        confirm_destructive,
        production: dexo_app::Environment::parse_strict(&model.connection.environment)
            == dexo_app::Environment::Production,
    }
}

/// The server said where a statement failed: underline it there, and put the cursor on
/// it -- if the document still reads as it did when the statement ran.
fn point_at_failure(
    model: &mut Model,
    key: &crate::runtime::OperationKey,
    index: usize,
    message: &str,
    position: Option<u32>,
) {
    let Some((offset, revision)) = model
        .results
        .tabs
        .get(index)
        .filter(|tab| tab.key.operation == *key)
        .and_then(|tab| tab.source_offset)
    else {
        return;
    };
    let Some(document) = model
        .documents
        .iter()
        .position(|document| document.id == key.document)
    else {
        return;
    };
    let doc = &model.documents[document];
    if doc.sql.revision() != revision {
        return;
    }
    let text = doc.text();
    // MySQL and SQLite name no position: the cursor still goes to the statement that
    // failed, as it does where the server points inside it.
    let Some(position) = position.filter(|position| *position > 0) else {
        let cursor = text[..offset.min(text.len())].chars().count();
        let doc = &mut model.documents[document];
        doc.anchor = None;
        let _ = doc.sql.set_cursor(cursor);
        if document == model.active_document {
            crate::screens::editor::follow_cursor(model);
        }
        return;
    };
    let Some(within) = text[offset.min(text.len())..]
        .char_indices()
        .nth(position as usize - 1)
        .map(|(at, _)| at)
    else {
        return;
    };
    let at = offset + within;
    // The token ends at a space or at the semicolon that ends the statement: the
    // underline covered the `;`.
    let end = text[at..]
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || *ch == ';')
        .map_or(text.len(), |(width, _)| at + width)
        .max(at + text[at..].chars().next().map_or(0, char::len_utf8));
    let mut diagnostic = dexo_sql::Diagnostic::server(message, "", None);
    diagnostic.byte_range = Some(at..end);
    model.editor.server_diagnostic = Some((doc.id.clone(), revision, diagnostic));
    let cursor = text[..at].chars().count();
    let doc = &mut model.documents[document];
    doc.anchor = None;
    let _ = doc.sql.set_cursor(cursor);
    if document == model.active_document {
        crate::screens::editor::follow_cursor(model);
    }
}

/// A run that changed the schema has ended, after its statements up to `failed_at`:
/// the tables they created are known from now on, and the catalog -- explorer and
/// diagnostics both -- is read again so the rest of the change shows too.
fn finish_schema_run(
    model: &mut Model,
    operation: crate::runtime::OperationId,
    failed_at: Option<usize>,
) -> Vec<Effect> {
    let Some(run) = model.schema_run.take_if(|run| run.operation == operation) else {
        return Vec::new();
    };
    let owner = (model.active_session, model.session_generation);
    if (model.session_tables.0, model.session_tables.1) != owner {
        model.session_tables = (owner.0, owner.1, Default::default());
    }
    let ran = failed_at.unwrap_or(run.created.len());
    // In order: a table created and dropped in one run is gone at its end.
    for (created, dropped) in run.created.into_iter().zip(run.dropped).take(ran) {
        model.session_tables.2.extend(created);
        for table in dropped {
            model.session_tables.2.remove(&table);
        }
    }
    let sql = model.active_document().text();
    let cursor = model.active_document().byte_cursor();
    crate::screens::editor::refresh_diagnostics(model, &sql, cursor);
    if model.active_session.is_none() || model.connection.name.is_empty() {
        return Vec::new();
    }
    reload_connection_tree(model)
}

fn launch_script(model: &mut Model, statements: Vec<String>) -> Vec<Effect> {
    // `\x` alone only turns the record view on or off: nothing runs, and the result on
    // screen is left where it is instead of being emptied for an answer that has no rows.
    if let [only] = statements.as_slice()
        && let Ok(dexo_app::meta_command::MetaCommand::RecordView(on)) =
            dexo_app::meta_command::parse(only)
    {
        return update(
            model,
            match on {
                Some(on) => Action::SetRecordView(on),
                None => Action::ToggleRecordView,
            },
        );
    }
    let operation = crate::runtime::OperationId::new();
    let dialect = crate::screens::editor::editor_dialect(model);
    let created: Vec<Option<String>> = statements
        .iter()
        .map(|sql| dexo_sql::created_table(sql, dialect))
        .collect();
    let dropped: Vec<Vec<String>> = statements
        .iter()
        .map(|sql| dexo_sql::dropped_tables(sql, dialect))
        .collect();
    let changes_schema = statements.iter().any(|sql| {
        dexo_sql::split_statements_in(sql, dialect)
            .iter()
            .any(|span| span.effect == dexo_sql::StatementEffect::SchemaWrite)
    });
    model.schema_run = (changes_schema || created.iter().any(Option::is_some)).then_some(
        crate::model::SchemaRun {
            operation,
            created,
            dropped,
        },
    );
    model.sql_transactions = Some(crate::model::SqlTransactions {
        operation,
        session: model.active_session,
        steps: statements
            .iter()
            .map(|sql| transaction_after(sql))
            .collect(),
    });
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
    // Where each statement sits in the document, so a failure the server places can be
    // shown in the text: where the run planned them from, when it planned these. A
    // statement not in it (from history) has none.
    let revision = model.active_document().sql.revision();
    let planned = crate::screens::workbench::planned_statement_spans(model);
    let from_document = planned.len() == statements.len()
        && planned
            .iter()
            .zip(&statements)
            .all(|((_, planned), statement)| planned == statement);
    let offsets: Vec<Option<(usize, u64)>> = (0..statements.len())
        .map(|index| from_document.then(|| (planned[index].0, revision)))
        .collect();
    model.editor.server_diagnostic = None;
    model.data.bars = crate::screens::data::ClauseBars::default();
    let dropped = drop_count(model);
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
            // Only a plain read can run again with a WHERE, an ORDER BY or a count; a
            // result of anything else keeps no statement to run, and shows no bars.
            tab.source_sql = dexo_sql::is_read(sql, dialect).then(|| sql.clone());
            tab.source_offset = offsets[index];
            tab
        })
        .collect();
    model.results.active = 0;
    let request = QueryRequest::read(statements[0].clone(), 10_000);
    model.active_query = Some(request.id);
    model.active_operation = Some(operation);
    let mut effects = checkpoint_dirty(model);
    effects.extend(dropped);
    effects.push(Effect::StartScript(crate::action::ScriptRequest {
        key,
        statements,
        dialect,
        policy: model.script_policy,
        parameters: Vec::new(),
        named: model
            .editor
            .parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.value.clone()))
            .collect(),
        timeout: std::time::Duration::from_secs(30),
        read_only: false,
    }));
    effects
}

/// The transaction state a statement leaves behind when it is one that opens or closes a
/// transaction, `None` for any other. `ROLLBACK TO` and `RELEASE` end a savepoint, not
/// the transaction.
fn transaction_after(sql: &str) -> Option<TransactionState> {
    let lowered = sql.trim().trim_end_matches(';').to_ascii_lowercase();
    let mut words = lowered.split_whitespace();
    match words.next()? {
        "begin" | "start" => Some(TransactionState::Active),
        "commit" | "end" | "abort" => Some(TransactionState::Idle),
        "rollback" if words.next() == Some("to") => None,
        "rollback" => Some(TransactionState::Idle),
        _ => None,
    }
}

/// Records what the statements of the finished run (those before `until`) did to the
/// transaction: a `begin;` typed in the editor is as open as one from the palette, and
/// quitting has to ask about it.
fn apply_sql_transactions(model: &mut Model, operation: crate::runtime::OperationId, until: usize) {
    let Some(run) = model
        .sql_transactions
        .take_if(|run| run.operation == operation)
    else {
        return;
    };
    let Some(state) = run.steps.iter().take(until).rev().find_map(|step| *step) else {
        return;
    };
    let Some(session) = run.session else {
        return;
    };
    if let Some(row) = model
        .connections
        .sessions
        .iter_mut()
        .find(|row| row.id == session)
    {
        row.transaction = state;
    }
    if model.active_session == Some(session) {
        model.transaction = state;
    }
}

fn cancel_query(model: &mut Model) -> Vec<Effect> {
    match model.active_operation {
        Some(operation) => vec![Effect::CancelOperation(operation)],
        None => {
            // The palette says so for this command; the key stayed silent.
            model.messages.warn("No query is running.".into());
            Vec::new()
        }
    }
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
        let titles: Vec<String> = recovery
            .documents
            .iter()
            .map(|document| document.title.clone())
            .collect();
        restore_recovery_documents(model, recovery.documents);
        // Said, not done silently: nothing told the user the documents were recovered.
        model.messages.info(format!(
            "Dexo closed unexpectedly. Restored {} unsaved document{}; Ctrl+P, Session Recovery lists them.",
            titles.len(),
            if titles.len() == 1 { "" } else { "s" }
        ));
        model.recovery.documents = titles;
    }
    let layout = if should_recover {
        recovery.layout.or(state.layout)
    } else {
        state.layout
    };
    model.connections.load_profiles(state.connections);
    apply_layout(model, layout);
    name_front_document_connection(model);
    focus_explorer_when_nothing_is_open(model);
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
        // The binding that was saved comes back with the document, and a recovered
        // document is unsaved by definition: it is marked, and asks before it closes.
        recovered.saved_revision = recovered.sql.revision().wrapping_add(1);
        if let Some(existing) = model
            .documents
            .iter_mut()
            .find(|document| document.id == recovered.id)
        {
            recovered.path = existing.path.clone();
            recovered.connection_id = existing.connection_id.clone();
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
    // The store keeps the text a document had when the app closed, not whether it was
    // saved. A draft with no file is unsaved by definition; one with a file is, when the
    // file says something else. Coming back as saved let Ctrl+W throw the text away.
    let unsaved = match &document.path {
        None => !stored.content.is_empty(),
        Some(path) => std::fs::read_to_string(path).is_ok_and(|disk| disk != stored.content),
    };
    if unsaved {
        document.sql.mark_modified();
    }
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

/// The header and the status bar name the connection of the document in front. The
/// layout remembers the connection that was active when the app closed, which need not
/// be that one: they said `pg-dev` over a document of `sqlite-shop`.
fn name_front_document_connection(model: &mut Model) {
    let name = model
        .documents
        .get(model.active_document)
        .and_then(|document| document.connection_id.as_deref())
        .and_then(|id| profile_by_uuid(model, id))
        .map(|profile| profile.name);
    if let Some(name) = name {
        model.connection.name = name;
    }
}

fn apply_layout(model: &mut Model, layout: Option<dexo_storage::WorkbenchLayout>) {
    let Some(layout) = layout else {
        return;
    };
    // As saved, not fitted to this terminal: it may be a small one for a moment, and
    // what is applied here is what the next save writes back.
    model.panes.explorer_visible = layout.explorer_visible;
    model.panes.results_visible = layout.results_visible;
    model.panes.explorer_width = layout.explorer_width.max(8);
    model.panes.results_height = layout.results_height.max(3);
    model.panes.console_height = layout.console_height.max(3);
    if let Some(id) = &layout.active_document_id
        && let Some(index) = model
            .documents
            .iter()
            .position(|document| &document.id == id)
    {
        model.active_document = index;
    }
    // A temporary connection from the last run, or one deleted since, is not coming
    // back; naming it would leave the workbench "offline" to nothing.
    if let Some(name) = layout.active_connection_id
        && model
            .connections
            .profiles
            .iter()
            .any(|row| row.profile.name == name)
    {
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

/// The output and the clause bars of `key`'s document: on screen, or parked with it
/// while another document is active.
fn document_output<'a>(
    model: &'a mut Model,
    key: &crate::runtime::OperationKey,
) -> Option<(
    &'a mut crate::model::ResultsState,
    &'a mut crate::screens::data::ClauseBars,
)> {
    if operation_matches(model, key) {
        return Some((&mut model.results, &mut model.data.bars));
    }
    let active = model.active_document;
    let (_, document) = model
        .documents
        .iter_mut()
        .enumerate()
        .find(|(index, document)| *index != active && document.id == key.document)?;
    Some((&mut document.results, &mut document.browse.bars))
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
            persist: !model.connections.is_temporary(&model.connection.name),
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
    let node_connection = model.explorer.selected_connection_name().map(str::to_owned);
    let mut effects = open_object_data(model);
    // The table's own details are asked of the session it lives on: while that one is
    // still being dialled, the live session is another connection's.
    if node_connection.is_none_or(|name| name == model.connection.name) {
        effects.extend(load_inspector(model));
        if let Some(id) = model.explorer.selected.clone() {
            let operation = crate::runtime::OperationId::new();
            if model.explorer.expand_with(&id, operation) {
                effects.extend(catalog_load_effect(model, Some(id), operation, false));
            }
        }
    }
    if effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::LoadTableData { .. } | Effect::ConnectProfile { .. }
        )
    }) {
        model.focus = Focus::Results;
        model.panes.results_visible = true;
    }
    effects
}

/// The connection's top level and every node open under it, read again: the tree stays
/// as it is -- open, and with the selection where it was.
fn reload_connection_tree(model: &mut Model) -> Vec<Effect> {
    let connection = crate::screens::explorer::connection_id(&model.connection.name);
    let mut effects = Vec::new();
    for parent in
        std::iter::once(connection.clone()).chain(model.explorer.expanded_under(&connection))
    {
        effects.extend(catalog_load_effect(
            model,
            Some(parent),
            crate::runtime::OperationId::new(),
            false,
        ));
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
    // The connection, a folder, or "all": a folder's id is not one the driver reads (it
    // answered with an empty list and emptied the folder), so each of these reads the
    // whole open tree of the connection again.
    let whole = all
        || model.explorer.selected_node().is_some_and(|node| {
            crate::screens::explorer::is_folder_node(node)
                || crate::screens::explorer::is_connection_node(node)
        });
    if whole {
        if model.connection.name.is_empty() {
            return Vec::new();
        }
        model.messages.info(format!(
            "Refreshing the catalog of {}.",
            model.connection.name
        ));
        return reload_connection_tree(model);
    }
    let operation = crate::runtime::OperationId::new();
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    model.explorer.expand_with(&id, operation);
    model.messages.info("Refreshing the selected node.".into());
    catalog_load_effect(model, Some(id), operation, false)
}

fn discard_all_pending(model: &mut Model) {
    // Edited cells show the old values again.
    let edited: Vec<usize> = model
        .data
        .row_changes
        .iter()
        .filter(|(_, state)| matches!(state, dexo_app::data::RowEditState::Edited))
        .map(|(&index, _)| index)
        .collect();
    for index in edited {
        undo_row_edit(model, index);
    }
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

/// Says why the grid's rows cannot be changed, when they cannot: the same answer, before
/// anything is typed or staged, for an insert, a delete and an edit. A connection's
/// read-only policy comes first -- a change staged on one could only be refused later.
fn edits_refused(model: &mut Model, verb: &str) -> bool {
    let target = model.data.target.clone();
    let name = target.object();
    let reason = if !model.active_document().kind.is_table() {
        format!("{verb} works on a table's rows: open one from the sidebar first.")
    } else if model.connection.read_only {
        format!(
            "{} is read-only: its rows cannot be changed here.",
            model.connection.name
        )
    } else if model.data.table.columns.is_empty() {
        format!("The columns of {name} are still loading; try again in a moment.")
    } else if model.data.changes.mode() == dexo_app::data::EditMode::ReadOnly {
        let is_view = model.catalog_objects.iter().any(|object| {
            object.qualified_name == target
                && matches!(
                    object.kind,
                    dexo_driver_api::ObjectKind::View
                        | dexo_driver_api::ObjectKind::MaterializedView
                )
        });
        if is_view {
            format!("{name} is a view, so its rows cannot be changed here.")
        } else {
            format!(
                "{name} has no primary key or unique column, so its rows cannot be changed here."
            )
        }
    } else {
        return false;
    };
    model.messages.warn(reason);
    true
}

/// Stands in for a document's id while `$EDITOR` holds a cell's value instead of a SQL
/// file: the finished edit comes back under it.
const CELL_EDIT_DOCUMENT: &str = "\u{0}cell-edit";

/// F2: the dialog to change the cell under the cursor. Every refusal says why, before
/// anything is typed: a connection that is read-only, a table whose rows have no key, a
/// column the row is found by, a row already going away.
fn open_cell_edit(model: &mut Model) -> Vec<Effect> {
    // F2 is Rename on a document's own panes, and only a table's cells are edited: on the
    // results of a statement it renames, as it does from everywhere else.
    if !model.active_document().kind.is_table() {
        return update(model, Action::RenameDocument);
    }
    if edits_refused(model, "Edit") {
        return Vec::new();
    }
    let Some((row, col)) = model.results.selection() else {
        return Vec::new();
    };
    let Some(column) = model.results.columns().get(col).cloned() else {
        return Vec::new();
    };
    match model.data.row_changes.get(&row) {
        Some(dexo_app::data::RowEditState::Deleted) => {
            model.messages.warn(
                "This row is marked for deletion; restore it with Delete to change it.".into(),
            );
            return Vec::new();
        }
        Some(dexo_app::data::RowEditState::Inserted) => {
            model.messages.warn(
                "This row is new and not applied yet: delete it and insert it again to change it."
                    .into(),
            );
            return Vec::new();
        }
        _ => {}
    }
    if dexo_app::data::RowIdentity::from_table(&model.data.table)
        .is_some_and(|identity| identity.contains(&column.name))
    {
        model.messages.warn(format!(
            "{} is what finds this row, so it cannot be changed here; change it with an UPDATE in the editor.",
            column.name
        ));
        return Vec::new();
    }
    // A value too big to hold in the grid is not edited from a prefix of it.
    if model.results.cell_at(row, col).is_some() {
        model.messages.warn(
            "This value is too large to edit here; change it with an UPDATE in the editor.".into(),
        );
        return Vec::new();
    }
    let value = model
        .results
        .rows()
        .get(row)
        .and_then(|cells| cells.get(col))
        .cloned()
        .unwrap_or(DbValue::Null);
    let was_null = matches!(value, DbValue::Null);
    let mut input = crate::widgets::text_input::TextInput::new(if was_null {
        String::new()
    } else {
        dexo_app::data::display_value(&value)
    });
    // Typing replaces the old value; an arrow key keeps it to be changed.
    input.select_all();
    model.data.cell_edit = Some(crate::screens::data::CellEditForm {
        row,
        column: col,
        table: model.data.target.object().to_string(),
        name: column.name,
        type_name: column.type_name,
        value: input,
        was_null,
        focus: crate::screens::data::CellFocus::Value,
    });
    Vec::new()
}

fn cell_edit_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::data::CellFocus;
    let Some(form) = model.data.cell_edit.as_mut() else {
        return Vec::new();
    };
    // The two accelerators work from anywhere in the dialog.
    if key.modifiers == KeyModifiers::CONTROL {
        match key.code {
            KeyCode::Char('n') => return stage_cell_edit(model, DbValue::Null),
            KeyCode::Char('e') => return cell_edit_externally(model),
            _ => {}
        }
    }
    match key.code {
        KeyCode::Esc => model.data.cell_edit = None,
        KeyCode::Tab | KeyCode::Down => form.focus = form.focus.next(),
        KeyCode::BackTab | KeyCode::Up => form.focus = form.focus.prev(),
        KeyCode::Left | KeyCode::Right if form.focus != CellFocus::Value => {
            form.focus = form.focus.step_button(key.code == KeyCode::Right);
        }
        KeyCode::Enter => match form.focus {
            CellFocus::Cancel => model.data.cell_edit = None,
            CellFocus::Null => return stage_cell_edit(model, DbValue::Null),
            CellFocus::Editor => return cell_edit_externally(model),
            CellFocus::Value | CellFocus::Save => return submit_cell_edit(model),
        },
        _ if form.focus == CellFocus::Value => {
            form.value.handle_key(key);
        }
        _ => {}
    }
    Vec::new()
}

fn mouse_cell_edit(model: &mut Model, hit: Option<HitTarget>) -> Vec<Effect> {
    use crate::screens::data::CellFocus;
    let Some(form) = model.data.cell_edit.as_mut() else {
        return Vec::new();
    };
    match hit {
        Some(HitTarget::FormField(_)) => form.focus = CellFocus::Value,
        Some(HitTarget::FooterSubmit) => return submit_cell_edit(model),
        Some(HitTarget::FooterCancel) => model.data.cell_edit = None,
        Some(HitTarget::Button(HitButton::SetNull)) => {
            return stage_cell_edit(model, DbValue::Null);
        }
        Some(HitTarget::Button(HitButton::OpenEditor)) => return cell_edit_externally(model),
        _ => {}
    }
    Vec::new()
}

/// Hands the value to `$EDITOR`; what it saves comes back into the dialog's field.
fn cell_edit_externally(model: &mut Model) -> Vec<Effect> {
    let Some(form) = &model.data.cell_edit else {
        return Vec::new();
    };
    model.external_edit = Some(crate::model::ExternalEdit {
        document: CELL_EDIT_DOCUMENT.into(),
        text: form.value.as_str().to_string(),
    });
    Vec::new()
}

/// What `$EDITOR` saved, into the dialog's field. An editor ends a file with a line break
/// the value did not have: one is dropped, as a commit message's is.
fn finish_cell_edit_externally(model: &mut Model, saved: Result<String, String>) {
    let Some(form) = model.data.cell_edit.as_mut() else {
        return;
    };
    match saved {
        Ok(text) => {
            let had_break = form.value.as_str().ends_with('\n');
            let text = match text.strip_suffix('\n') {
                Some(stripped) if !had_break => stripped.to_string(),
                _ => text,
            };
            form.value.set_text(text);
            form.focus = crate::screens::data::CellFocus::Save;
        }
        Err(message) => model.messages.error(message),
    }
}

fn submit_cell_edit(model: &mut Model) -> Vec<Effect> {
    let Some(form) = &model.data.cell_edit else {
        return Vec::new();
    };
    // An empty field is the empty string; NULL is its own button.
    let value = if form.was_null && form.value.is_empty() {
        DbValue::Null
    } else {
        DbValue::Text(form.value.as_str().to_string())
    };
    stage_cell_edit(model, value)
}

/// Stages `value` as the new content of the cell the dialog is on: an UPDATE in the change
/// set, for the review to show and apply. The row keeps one UPDATE however many of its
/// cells change, and a cell put back to what it was loaded with leaves none.
fn stage_cell_edit(model: &mut Model, value: DbValue) -> Vec<Effect> {
    let Some(form) = model.data.cell_edit.take() else {
        return Vec::new();
    };
    let (row, col) = (form.row, form.column);
    let Some(identity) = row_identity_at(model, row) else {
        model.messages.warn(
            "A column that identifies this row is not in the result, so it cannot be changed."
                .into(),
        );
        return Vec::new();
    };
    let existing = find_pending_index(
        &model.data.changes,
        |change| matches!(change, dexo_app::data::PendingChange::Update { identity: other, .. } if other == &identity),
    );
    // The row as it was loaded, and what this row's UPDATE sets so far.
    let (loaded, mut set) = match existing.and_then(|at| model.data.changes.pending().get(at)) {
        Some(dexo_app::data::PendingChange::Update {
            original, values, ..
        }) => (original.clone(), values.clone()),
        _ => match row_original_at(model, row) {
            Some(original) => (original, Vec::new()),
            None => return Vec::new(),
        },
    };
    let name = form.name;
    let before = loaded
        .iter()
        .find(|(column, _)| *column == name)
        .map(|(_, value)| value.clone());
    set.retain(|(column, _)| *column != name);
    // Typed text that reads as the value already there is no change: `5` over 5.
    let same = match (&value, &before) {
        (DbValue::Null, Some(DbValue::Null)) => true,
        (DbValue::Null, _) | (_, Some(DbValue::Null)) | (_, None) => false,
        (value, Some(before)) => {
            dexo_app::data::display_value(value) == dexo_app::data::display_value(before)
        }
    };
    if !same {
        set.push((name, value.clone()));
    }
    if let Some(at) = existing {
        model.data.changes.revert(at);
    }
    if set.is_empty() {
        model.data.row_changes.remove(&row);
    } else {
        model.data.changes.update(identity, loaded, set);
        model
            .data
            .row_changes
            .insert(row, dexo_app::data::RowEditState::Edited);
    }
    model.results.set_cell(row, col, value);
    note_staged(model);
    Vec::new()
}

/// Tells the user a change is waiting, and how to send it: staging used to be silent.
fn note_staged(model: &mut Model) {
    let pending = model.data.changes.pending().len();
    if pending == 0 {
        return;
    }
    let key = crate::palette::shortcut_for(model, "data.review", None).unwrap_or("Ctrl+S".into());
    model.messages.info(format!(
        "{pending} pending {}: {key} to review and apply.",
        if pending == 1 { "change" } else { "changes" }
    ));
}

fn submit_insert_row(model: &mut Model) -> Vec<Effect> {
    // A value the column cannot take keeps the form open with the reason: it used to close
    // and queue a row that Apply refused, naming no column.
    if let Err(reason) = model.data.insert_form.check() {
        model.data.insert_form.error = Some(reason);
        return Vec::new();
    }
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
    // The new row is at the end of the page; the cursor goes to it, so it is seen.
    let col = model.results.selection().map_or(0, |(_, col)| col);
    model.results.select_cell(row_index, col);
    model.results.move_cursor_row(0, false);
    note_staged(model);
    Vec::new()
}

fn toggle_row_delete(model: &mut Model) -> Vec<Effect> {
    let Some(row_index) = model.results.cursor_row() else {
        return Vec::new();
    };
    // A row going away is not also a row changed: its edit is taken back first, the
    // grid showing the row as it was loaded.
    if model.data.row_changes.get(&row_index).copied() == Some(dexo_app::data::RowEditState::Edited)
    {
        undo_row_edit(model, row_index);
    }
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
            if edits_refused(model, "Delete") {
                return Vec::new();
            }
            let Some(identity) = row_identity_at(model, row_index) else {
                model.messages.warn(
                    "A column that identifies this row is not in the result, so it cannot be deleted."
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
            note_staged(model);
        }
    }
    Vec::new()
}

/// Takes back the changes staged on a row's cells: the UPDATE goes, and the grid shows the
/// values the row was loaded with.
fn undo_row_edit(model: &mut Model, row_index: usize) {
    let Some(identity) = row_identity_at(model, row_index) else {
        return;
    };
    let existing = find_pending_index(
        &model.data.changes,
        |change| matches!(change, dexo_app::data::PendingChange::Update { identity: other, .. } if other == &identity),
    );
    if let Some(at) = existing
        && let Some(dexo_app::data::PendingChange::Update { original, .. }) =
            model.data.changes.pending().get(at).cloned()
    {
        model.data.changes.revert(at);
        for (name, value) in original {
            if let Some(col) = model
                .results
                .columns()
                .iter()
                .position(|column| column.name == name)
            {
                model.results.set_cell(row_index, col, value);
            }
        }
    }
    model.data.row_changes.remove(&row_index);
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
    // Through the model, not by assignment: the output pane and the table's paging move to
    // the document now, so what the load below writes is its own and no other's.
    model.set_active_document(index);
    if model.documents[index].kind.is_table() {
        return load_table_on_its_connection(model, index, false);
    }
    match switch_to_document_connection(model, index) {
        Switch::Ready => Vec::new(),
        Switch::Activated(effects) | Switch::Dialling(effects) => effects,
    }
}

/// The open document of `target` on the live connection, to open it in again: every
/// SQLite file's tables are `main.x`, so the name alone would take another
/// connection's. A related row's hop is its own, and never reused.
fn document_index_for_table(
    model: &Model,
    target: &dexo_driver_api::QualifiedName,
    connection: &Option<String>,
) -> Option<usize> {
    model.documents.iter().position(|document| {
        matches!(&document.kind, crate::model::DocumentKind::Table(existing) if existing == target)
            && &document.connection_id == connection
            && document.related_from.is_none()
    })
}

/// The connection a sidebar node belongs to, as a document records it: the one whose
/// tree the node hangs in, which is not always the live one.
fn node_connection_uuid(model: &Model) -> Option<String> {
    let name = model.explorer.selected_connection_name()?;
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| row.profile.id.0.to_string())
}

/// Opens the selected table's rows on the connection of the node it was picked in: the
/// guards -- read-only, production -- are that connection's, and so is the session the
/// rows come from.
fn open_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(node) = model.explorer.selected_node() else {
        return Vec::new();
    };
    let target = dexo_app::parse_qualified(&node.qualified);
    let connection_id = node_connection_uuid(model).or_else(|| active_connection_uuid(model));
    if model.active_session.is_none() && connection_id.is_none() {
        model
            .messages
            .warn("connect a session to browse table data".into());
        return Vec::new();
    }
    let index = match document_index_for_table(model, &target, &connection_id) {
        Some(index) => index,
        None => {
            model
                .documents
                .push(crate::model::EditorDocument::new_table(
                    target,
                    connection_id,
                ));
            model.documents.len() - 1
        }
    };
    // Bringing the session over re-selects the connection in the tree; the table the
    // user picked stays the selected node.
    let picked = model.explorer.selected.clone();
    model.set_active_document(index);
    model.data.last_error = None;
    let effects = load_table_on_its_connection(model, index, true);
    if let Some(picked) = picked {
        model.explorer.select(picked);
    }
    effects
}

/// Loads a table document's rows on its own connection: the live session moves there
/// first, or is dialled, in which case the load runs once the dial has landed.
/// `reload` loads rows already on screen again; without it a document that has rows
/// keeps them.
fn load_table_on_its_connection(model: &mut Model, index: usize, reload: bool) -> Vec<Effect> {
    let mut effects = match switch_to_document_connection(model, index) {
        Switch::Ready => Vec::new(),
        Switch::Activated(effects) => effects,
        Switch::Dialling(effects) => {
            model.pending_execute = Some(crate::model::PendingExecute {
                document: model.documents[index].id.clone(),
                action: Action::RefreshTableData,
                token: model.connect_token,
            });
            return effects;
        }
    };
    if reload || model.results.columns().is_empty() {
        effects.extend(load_table_document(model, index));
    }
    effects
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
            .push(format!("[{}] Connected", crate::model::clock()));
    }
    match crate::runtime::data_manager::table_request(
        target.clone(),
        Vec::new(),
        model.data.filter.clone(),
        Vec::new(),
        model.data.page_offset,
        model.data.page_limit,
        model.data.bars.applied.clone(),
    ) {
        Ok(request) => {
            // What runs, filters and all: the log said a bare SELECT for rows a foreign
            // key or a WHERE had narrowed.
            let mut shown = format!("SELECT * FROM {}", target.display_unquoted());
            let typed = request
                .filter
                .as_ref()
                .map(crate::screens::data_browser::describe_filter);
            if let Some(condition) = request.clauses.condition(typed) {
                shown.push_str(&format!(" WHERE {condition}"));
            }
            if let Some(order) = request.clauses.order() {
                shown.push_str(&format!(" ORDER BY {order}"));
            }
            shown.push_str(&format!(" LIMIT {}", request.page.limit));
            if request.page.offset > 0 {
                shown.push_str(&format!(" OFFSET {}", request.page.offset));
            }
            // The values a filter compares with are sent bound, not in the text.
            let bound = if request.filter.is_some() {
                "  -- values bound"
            } else {
                ""
            };
            model.documents[index].console_log.push(format!(
                "[{}] {}> {shown}{bound}",
                crate::model::clock(),
                target.display_unquoted(),
            ));
            let (page, columns) = (
                crate::runtime::OperationId::new(),
                crate::runtime::OperationId::new(),
            );
            model.data.page_ticket = Some(page);
            model.data.columns_ticket = Some(columns);
            vec![
                Effect::LoadTableData {
                    request,
                    session,
                    generation: model.session_generation,
                    ticket: page,
                },
                Effect::LoadTableColumns {
                    target,
                    session,
                    generation: model.session_generation,
                    ticket: columns,
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
        "[{}] {row_count} {} retrieved starting from {} in {elapsed_ms} ms",
        crate::model::clock(),
        if row_count == 1 { "row" } else { "rows" },
        offset + 1
    ));
}

/// Whether the grid pages at all, and whether there is a page the way `n` or `p` goes. It
/// says so when there is not: a key that does nothing looks like one that is broken.
/// Only a table's rows, or a result run again with the bars, are in pages.
fn at_page_edge(model: &mut Model, forward: bool) -> bool {
    if model.active_session.is_none() {
        return false;
    }
    let tab = model.results.tabs.get(model.results.active);
    let paged = tab.is_some_and(|tab| tab.paged);
    if !model.active_document().kind.is_table() && !paged {
        let message = if tab.is_some_and(|tab| tab.truncated) {
            "Only the first rows of this result are loaded. Narrow it with a WHERE (w) to reach the rest."
        } else {
            "This result is not in pages: every row it returned is loaded."
        };
        model.messages.info(message.into());
        return true;
    }
    if !forward && model.data.page_offset == 0 {
        model.messages.info("This is the first page.".into());
        return true;
    }
    // A paged result says it is on its last page in `change_data_page`, which knows how
    // many rows the page came back with.
    if forward && model.active_document().kind.is_table() && !model.data.has_more {
        model.messages.info("This is the last page.".into());
        return true;
    }
    false
}

fn change_data_page(model: &mut Model, offset: u64) -> Vec<Effect> {
    if model.active_session.is_none() {
        model.data.last_error = Some("connect a session first".into());
        return Vec::new();
    }
    // A result run again with the bars is paged too: the statement runs again at the
    // next offset.
    let paged = model
        .results
        .tabs
        .get(model.results.active)
        .filter(|tab| tab.paged)
        .and_then(|tab| tab.source_sql.clone());
    if let Some(source) = paged {
        let shown = model.results.row_count() as u64;
        if offset > model.data.page_offset && shown < u64::from(model.data.page_limit) {
            model.messages.info("This is the last page.".into());
            return Vec::new();
        }
        if rerun_refused(model) {
            return Vec::new();
        }
        model.data.page_offset = offset;
        return rerun_derived(model, source);
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
    // Read again, the rows may be others: a count of the old ones no longer holds.
    let mut effects = drop_count(model);
    // A table whose columns never arrived -- opened while its connection was still being
    // dialled, or restored at launch -- is opened whole, not just paged.
    if model.data.table.columns.is_empty() {
        effects.extend(load_table_document(model, model.active_document));
    } else {
        effects.extend(change_data_page(model, model.data.page_offset));
    }
    effects
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

/// Whether the bars are on screen: the grid view of rows that can run again, not the
/// one-record-per-block view.
fn bars_drawn(model: &Model) -> bool {
    model.results.view == crate::model::ResultsView::Grid
        && !(model.expanded_records && model.results.row_count() > 0)
        && clause_bars_shown(model)
}

/// The bar that has the keys: one that is focused, drawn, and in the focused pane.
pub(crate) fn focused_bar(model: &Model) -> Option<crate::screens::data::ClauseBar> {
    model
        .data
        .bars
        .focus
        .filter(|_| model.effective_focus() == Focus::Results && bars_drawn(model))
}

/// Whether the grid can run again with a WHERE and an ORDER BY: a table's rows, or the
/// result of a statement Dexo knows.
pub(crate) fn clause_bars_shown(model: &Model) -> bool {
    model.active_document().kind.is_table()
        || model
            .results
            .tabs
            .get(model.results.active)
            .is_some_and(|tab| tab.source_sql.is_some())
}

/// The keys the bars own while one has the focus: typing, Enter to run with them, Esc
/// to go back to what last ran, Tab to the other bar. Ctrl and Alt chords go on to the
/// keymap.
fn clause_bar_key(model: &mut Model, key: KeyEvent) -> Option<Vec<Effect>> {
    use crate::screens::data::ClauseBar;
    // A bar keeps the keys only while it is drawn and its pane has the focus; once
    // either goes, so does the bar's focus, and the keys go where they are meant to.
    let Some(bar) = focused_bar(model) else {
        model.data.bars.focus = None;
        return None;
    };
    // Ctrl+A, Ctrl+W and the word keys are the bar's: the keymap had them first, so
    // Ctrl+A selected nothing and Ctrl+W closed the table's tab.
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        && !crate::widgets::text_input::TextInput::owns(&key)
    {
        return None;
    }
    match key.code {
        KeyCode::Esc => model.data.bars.revert(),
        KeyCode::Enter => return Some(apply_clauses(model)),
        KeyCode::Tab | KeyCode::BackTab => {
            model.data.bars.focus = Some(match bar {
                ClauseBar::Where => ClauseBar::Order,
                ClauseBar::Order => ClauseBar::Where,
            });
        }
        _ => {
            model.data.bars.input_mut(bar).handle_key(key);
            model.data.bars.failed = false;
        }
    }
    Some(Vec::new())
}

/// Whether the grid cannot run again now: a statement of this session is still running
/// (a re-run would take its place, out of Ctrl+F2's reach), or row edits are pending
/// (a reload would pin each to whatever row lands at its index). Says why.
fn rerun_refused(model: &mut Model) -> bool {
    if model.active_operation.is_some() {
        model.messages.warn(
            "A statement is still running; wait for it, or cancel it, before sorting or filtering."
                .into(),
        );
        return true;
    }
    reload_would_orphan_edits(model)
}

/// `t`: counts the rows the grid pages through, exactly; `t` while it runs cancels it.
/// A table's count runs on a connection of its own, so the live query keeps its slot; a
/// result's runs on the session it came from. Offline, the document's connection is
/// dialled first.
fn count_rows(model: &mut Model) -> Vec<Effect> {
    use crate::screens::data::{CountState, RowCount};
    if let Some(effects) = when_connected(model, Action::CountRows) {
        return effects;
    }
    let key = count_key(model);
    let mut effects = Vec::new();
    if let Some(RowCount {
        key: counting,
        state: CountState::Running(operation),
    }) = model.data.count.clone()
    {
        model.data.count = None;
        effects.push(Effect::CancelCount { operation });
        // The same rows: `t` again cancels. Other rows -- a WHERE changed since -- are
        // counted in its place.
        if Some(&counting) == key.as_ref() {
            model.messages.info("Count cancelled.".into());
            return effects;
        }
    }
    let Some(session) = model.active_session else {
        model
            .messages
            .warn("Connect a session to count the rows.".into());
        return effects;
    };
    let (Some(key), Some(sql)) = (key, count_sql(model)) else {
        model.messages.warn(
            "Counting needs a table's rows, or the result of a statement that only reads.".into(),
        );
        return effects;
    };
    let on_session = !model.active_document().kind.is_table();
    if on_session && model.active_operation.is_some() {
        model
            .messages
            .warn("A statement is still running; count when it finishes, or cancel it.".into());
        return effects;
    }
    let operation = crate::runtime::OperationId::new();
    model.data.count = Some(RowCount {
        key,
        state: CountState::Running(operation),
    });
    effects.push(Effect::CountRows {
        session,
        operation,
        sql,
        parameters: model
            .data
            .filter
            .as_ref()
            .map(dexo_sql::filter_values)
            .unwrap_or_default(),
        on_session,
    });
    effects
}

/// What the grid pages through, as a count compares it: see [`CountKey`].
///
/// [`CountKey`]: crate::screens::data::CountKey
pub(crate) fn count_key(model: &Model) -> Option<crate::screens::data::CountKey> {
    let source = if model.active_document().kind.is_table() {
        model.data.target.display_unquoted()
    } else {
        model
            .results
            .tabs
            .get(model.results.active)?
            .source_sql
            .clone()?
    };
    Some(crate::screens::data::CountKey {
        source,
        filter: model.data.filter.clone(),
        clauses: model.data.bars.applied.clone(),
    })
}

/// The count the grid shows no longer holds: its rows were read again or changed. One
/// still running is stopped.
fn drop_count(model: &mut Model) -> Vec<Effect> {
    use crate::screens::data::CountState;
    match model.data.count.take().map(|count| count.state) {
        Some(CountState::Running(operation)) => vec![Effect::CancelCount { operation }],
        _ => Vec::new(),
    }
}

/// An action that needs the document's session: `None` to go on now -- the session is
/// live, or there is no connection to dial -- else what dials the document's connection,
/// after which the action runs. Offline actions connect by themselves.
fn when_connected(model: &mut Model, action: Action) -> Option<Vec<Effect>> {
    if model.active_session.is_some() {
        return None;
    }
    match switch_to_document_connection(model, model.active_document) {
        Switch::Ready => None,
        Switch::Activated(mut effects) => {
            effects.extend(update(model, action));
            Some(effects)
        }
        Switch::Dialling(effects) => {
            model.pending_execute = Some(crate::model::PendingExecute {
                document: model.active_document().id.clone(),
                action,
                token: model.connect_token,
            });
            Some(effects)
        }
    }
}

/// The `SELECT COUNT(*)` for what the grid pages through: a table document's rows, or
/// the statement a result came from, under the WHERE that ran.
pub(crate) fn count_sql(model: &Model) -> Option<String> {
    let dialect = crate::screens::editor::editor_dialect(model);
    if model.active_document().kind.is_table() {
        return dexo_sql::table_count_in(
            &model.data.target,
            &model.data.filter,
            &model.data.bars.applied,
            dialect,
        )
        .ok();
    }
    // Only a plain read keeps its statement (see `launch_script`).
    let source = model
        .results
        .tabs
        .get(model.results.active)?
        .source_sql
        .clone()?;
    dexo_sql::derive_count_in(
        &source,
        &model.data.filter,
        &model.data.bars.applied,
        dialect,
    )
    .ok()
}

/// A header click, or `s`: the ORDER BY bar's text is the sort, so the click rewrites
/// it and runs the grid again with it. Text no header can show -- an expression -- is
/// replaced. The WHERE that runs is the one that last ran, not one half typed.
fn sort_by_column(model: &mut Model, column: usize, add: bool) -> Vec<Effect> {
    if !clause_bars_shown(model) {
        model
            .messages
            .warn("Sorting runs a table's rows or a query's result again; run one first.".into());
        return Vec::new();
    }
    let Some(name) = model
        .results
        .columns()
        .get(column)
        .map(|meta| meta.name.clone())
    else {
        return Vec::new();
    };
    if rerun_refused(model) {
        return Vec::new();
    }
    let dialect = crate::screens::editor::editor_dialect(model);
    let applied = model.data.bars.applied.order_by.clone().unwrap_or_default();
    let keys = dexo_sql::order_keys(&applied, dialect).unwrap_or_default();
    let keys = dexo_sql::cycle_order(&keys, &name, add);
    let bars = &mut model.data.bars;
    bars.where_input
        .set_text(bars.applied.where_sql.clone().unwrap_or_default());
    bars.order_input
        .set_text(dexo_sql::order_text(&keys, dialect));
    apply_clauses(model)
}

/// Runs the grid again with the bars' text, once it is known to only read. Refused text
/// stays in the bar to be fixed, and nothing is sent.
fn apply_clauses(model: &mut Model) -> Vec<Effect> {
    if rerun_refused(model) {
        return Vec::new();
    }
    let clauses = model.data.bars.typed();
    let dialect = crate::screens::editor::editor_dialect(model);
    if let Err(reason) = dexo_sql::clauses_read(&clauses, dialect) {
        model.messages.error(format!("Not applied: {reason}"));
        return Vec::new();
    }
    model.data.bars.applied = clauses;
    model.data.bars.focus = None;
    model.data.bars.failed = false;
    model.data.page_offset = 0;
    apply_remote_query(model)
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
    // Sorting or filtering runs the statement again. One that is not a plain read -- a
    // side-effecting function, a locking read -- was confirmed once, if at all, and is
    // sorted from the rows already loaded instead.
    if !dexo_sql::is_read(&sql, crate::screens::editor::editor_dialect(model)) {
        let reason = "the statement is not read-only, so Dexo does not run it again".to_string();
        if let Some(tab) = model.results.tabs.get_mut(model.results.active) {
            tab.local_only = Some(reason.clone());
        }
        model.messages.warn(format!("local-only: {reason}"));
        return Vec::new();
    }
    let page = match dexo_driver_api::Page::new(model.data.page_offset, model.data.page_limit) {
        Ok(page) => page,
        Err(error) => {
            model.messages.error(error.to_string());
            return Vec::new();
        }
    };
    let dialect = crate::screens::editor::editor_dialect(model);
    match dexo_sql::derive_page_in(
        &sql,
        &[],
        &model.data.filter,
        &model.data.bars.applied,
        page,
        dialect,
    ) {
        Ok(derived) => {
            if let Some(tab) = model.results.tabs.get_mut(model.results.active) {
                tab.local_only = None;
            }
            let mut parameters = Vec::new();
            if let Some(filter) = &model.data.filter {
                parameters = dexo_sql::filter_values(filter);
            }
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

fn start_derived_script(model: &mut Model, sql: String, parameters: Vec<DbValue>) -> Vec<Effect> {
    let operation = crate::runtime::OperationId::new();
    // Until the new run answers with rows, the result it replaces is kept: a clause the
    // server turns down puts it back.
    model.results.derived_backup = Some(crate::model::DerivedBackup {
        operation,
        tabs: model.results.tabs.clone(),
        active: model.results.active,
    });
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
    tab.paged = true;
    // The same statement, run again: the cursor stays in its column.
    tab.grid.home_column = model.results.selection().map_or(0, |(_, col)| col);
    model.results.replace_tabs(tab);
    model.active_operation = Some(operation);
    vec![Effect::StartScript(crate::action::ScriptRequest {
        key,
        statements: vec![sql],
        dialect: crate::screens::editor::editor_dialect(model),
        policy: model.script_policy,
        parameters,
        named: Vec::new(),
        timeout: std::time::Duration::from_secs(30),
        read_only: true,
    })]
}

fn reload_object_data(model: &mut Model) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    // The console says how long this page took, not how long since the table opened.
    model.data.request_started = Some(std::time::Instant::now());
    match crate::runtime::data_manager::table_request(
        model.data.target.clone(),
        Vec::new(),
        model.data.filter.clone(),
        Vec::new(),
        model.data.page_offset,
        model.data.page_limit,
        model.data.bars.applied.clone(),
    ) {
        Ok(request) => {
            let ticket = crate::runtime::OperationId::new();
            model.data.page_ticket = Some(ticket);
            vec![Effect::LoadTableData {
                request,
                session,
                generation: model.session_generation,
                ticket,
            }]
        }
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

/// `n` in the inspector: the note editor, holding the note as it stands.
fn start_note_editor(model: &mut Model) {
    let current = model.inspector.note.clone().unwrap_or_default();
    model.inspector.editing_note = Some((
        crate::widgets::text_input::TextInput::new(current),
        crate::widgets::form::FooterFocus::Input,
    ));
}

/// The palette's way to `n`: on the explorer's object, opening its inspector first when
/// it is not the one open. The editor opens once the object and its note are read.
fn edit_object_note(model: &mut Model) -> Vec<Effect> {
    if model.inspector.open && model.inspector.object.is_some() {
        start_note_editor(model);
        return Vec::new();
    }
    let effects = open_inspector_facet(
        model,
        crate::screens::object_inspector::InspectorFacet::Properties,
    );
    model.inspector.note_requested = !effects.is_empty();
    effects
}

/// Saves the note being written. It is shown once the save answers, not before: a
/// failed write showed a note that was never kept.
fn submit_note(model: &mut Model) -> Vec<Effect> {
    let Some((input, _)) = model.inspector.editing_note.take() else {
        return Vec::new();
    };
    let note = input.as_str().trim().to_string();
    let connection = active_connection_uuid(model).filter(|id| is_saved_connection(model, id));
    match (model.inspector.note_key(), connection) {
        (Some(object), Some(connection_id)) => vec![Effect::SaveNote {
            connection_id,
            object,
            note,
        }],
        _ => {
            model.messages.warn(
                "A note belongs to a saved connection's object; save this connection first (Save Connection…)."
                    .into(),
            );
            Vec::new()
        }
    }
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

/// Go To Definition: the object under the cursor is shown in the explorer when the tree
/// has it, and its DDL opens when only the catalog does. It only knew what the explorer
/// had expanded, and otherwise said "no definition at cursor" and nothing else.
fn goto_definition(model: &mut Model) -> Vec<Effect> {
    let sql = model.active_document().text();
    let cursor = model.active_document().byte_cursor();
    let whole_catalog =
        !model.catalog_objects.is_empty() && model.catalog_connection == model.connection.name;
    let objects = if whole_catalog {
        model.catalog_objects.clone()
    } else {
        flatten_explorer(&model.explorer)
    };
    let catalog = dexo_app::SnapshotCatalog::new(objects);
    let Some(target) = dexo_sql::definition_at(&sql, cursor, &catalog) else {
        model.messages.warn(
            "No definition here: put the cursor on a table or column the catalog knows.".into(),
        );
        return Vec::new();
    };
    let wanted = target.display_unquoted();
    if let Some(id) = find_qualified(&model.explorer, &wanted) {
        model.explorer.reveal(&id);
        model.explorer.select(id.clone());
        model.panes.explorer_visible = true;
        model
            .messages
            .info(format!("{wanted} is selected in the explorer."));
        let operation = crate::runtime::OperationId::new();
        if model.explorer.expand_with(&id, operation) {
            return catalog_load_effect(model, Some(id), operation, false);
        }
        return Vec::new();
    }
    // Not in the tree, whose schemas may be collapsed and never read: the catalog has it,
    // and its definition is the DDL.
    let found = model
        .catalog_objects
        .iter()
        .find(|object| {
            object.qualified_name.display_unquoted() == wanted
                && crate::screens::explorer::opens_table_data(&object.kind)
        })
        .map(|object| object.id.clone());
    match (found, model.active_session) {
        (Some(id), Some(session)) => {
            model.inspector = crate::screens::object_inspector::ObjectInspector::loading(&wanted);
            model.inspector.open = true;
            model.inspector.facet = crate::screens::object_inspector::InspectorFacet::Ddl;
            vec![Effect::LoadObjectInspector {
                id,
                session,
                generation: model.session_generation,
            }]
        }
        _ => {
            model.messages.warn(format!(
                "{wanted} is not in the explorer; expand its schema there, or refresh the catalog."
            ));
            Vec::new()
        }
    }
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
                show_value(
                    model,
                    inspect_value(&bytes_or_text(bytes), loaded.min(total), total),
                );
                return Vec::new();
            }
            crate::model::GridCell::Inline(value) => {
                show_value(model, crate::screens::value_viewer::view(&value));
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
    let view = crate::screens::value_viewer::view(value);
    show_value(model, view);
    Vec::new()
}

fn show_value(model: &mut Model, view: dexo_app::data::ValueView) {
    model.data.viewer = Some(view);
    model.data.viewer_scroll = 0;
}

/// A value fetched or spooled arrives as bytes; text that reads as text is shown as text,
/// not as the hex of its bytes.
fn bytes_or_text(bytes: Vec<u8>) -> DbValue {
    match String::from_utf8(bytes) {
        Ok(text)
            if !text
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')) =>
        {
            DbValue::Text(text)
        }
        Ok(text) => DbValue::Bytes(text.into_bytes()),
        Err(error) => DbValue::Bytes(error.into_bytes()),
    }
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

/// Save Query As: the selection when there is one, the document otherwise, under a
/// name, for this project and the document's connection.
fn open_save_query(model: &mut Model) -> Vec<Effect> {
    let document = model.active_document();
    if document.kind.is_table() || document.kind.is_placeholder() {
        model
            .messages
            .warn("Save Query As saves a SQL document's text; open one first.".into());
        return Vec::new();
    }
    let text = document.text();
    let (sql, source) = match model.editor_selection() {
        Some(range) => (
            text.chars()
                .skip(range.start)
                .take(range.end - range.start)
                .collect(),
            "the selection",
        ),
        _ => (text, "the whole document"),
    };
    let sql = sql.trim().to_string();
    if sql.is_empty() {
        model.messages.warn("There is no SQL to save.".into());
        return Vec::new();
    }
    let Some(connection_id) = query_connection(model) else {
        model
            .messages
            .warn("A saved query belongs to a connection; connect this document first.".into());
        return Vec::new();
    };
    if !is_saved_connection(model, &connection_id) {
        model.messages.warn(
            "A saved query belongs to a saved connection; save this one first (Save Connection…)."
                .into(),
        );
        return Vec::new();
    }
    let document = model.active_document();
    if model.project_id.is_empty() {
        model
            .messages
            .warn("Saved queries belong to a project; open one first.".into());
        return Vec::new();
    }
    let suggested = document
        .title
        .strip_suffix(".sql")
        .unwrap_or(&document.title)
        .to_string();
    let name = crate::widgets::text_input::TextInput::new(suggested);
    model.save_query_prompt = Some(crate::screens::saved_queries::SaveQueryPrompt {
        name,
        footer: crate::widgets::form::FooterFocus::Input,
        error: None,
        sql,
        connection_id,
        source,
    });
    Vec::new()
}

fn save_query_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, footer_key};
    let Some(prompt) = model.save_query_prompt.as_mut() else {
        return Vec::new();
    };
    match footer_key(&mut prompt.footer, &key) {
        FooterKey::Submit => submit_save_query(model),
        FooterKey::Cancel => {
            model.save_query_prompt = None;
            Vec::new()
        }
        FooterKey::Moved => Vec::new(),
        FooterKey::Pass => {
            if prompt.footer == crate::widgets::form::FooterFocus::Input {
                prompt.name.handle_key(key);
                prompt.error = None;
            }
            Vec::new()
        }
    }
}

/// Try index's keys: the definition is typed, Enter tries it, Esc closes.
fn try_index_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, footer_key};
    let Some(prompt) = model.try_index.as_mut() else {
        return Vec::new();
    };
    match footer_key(&mut prompt.footer, &key) {
        FooterKey::Submit => submit_try_index(model),
        FooterKey::Cancel => {
            model.try_index = None;
            Vec::new()
        }
        FooterKey::Moved => Vec::new(),
        FooterKey::Pass => {
            if prompt.footer == crate::widgets::form::FooterFocus::Input {
                prompt.input.handle_key(key);
            }
            Vec::new()
        }
    }
}

fn submit_try_index(model: &mut Model) -> Vec<Effect> {
    let Some(definition) = model
        .try_index
        .as_ref()
        .map(|prompt| prompt.input.trim().to_string())
    else {
        return Vec::new();
    };
    update(model, Action::TryIndex { definition })
}

fn submit_save_query(model: &mut Model) -> Vec<Effect> {
    let Some(prompt) = model.save_query_prompt.as_mut() else {
        return Vec::new();
    };
    let name = prompt.name.as_str().trim().to_string();
    if name.is_empty() {
        prompt.error = Some("A saved query needs a name.".into());
        return Vec::new();
    }
    let Some(prompt) = model.save_query_prompt.take() else {
        return Vec::new();
    };
    vec![Effect::SaveQuery {
        project_id: model.project_id.clone(),
        connection_id: prompt.connection_id,
        name,
        sql: prompt.sql,
    }]
}

/// The picker's keys: typing searches, Up and Down pick, Enter opens, F2 renames and
/// Delete asks before it deletes.
fn saved_queries_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, confirm_key};
    let project_id = model.project_id.clone();
    let picker = &mut model.saved_queries;
    if let Some(focus) = picker.deleting.as_mut() {
        match confirm_key(focus, &key) {
            FooterKey::Submit => {
                picker.deleting = None;
                if let Some(query) = picker.current() {
                    let id = query.id.clone();
                    return vec![Effect::DeleteSavedQuery { project_id, id }];
                }
            }
            FooterKey::Cancel => picker.deleting = None,
            FooterKey::Moved | FooterKey::Pass => {}
        }
        return Vec::new();
    }
    if let Some(input) = picker.renaming.as_mut() {
        match key.code {
            KeyCode::Esc => picker.renaming = None,
            KeyCode::Enter => {
                let name = input.as_str().trim().to_string();
                if name.is_empty() {
                    picker.error = Some("A saved query needs a name.".into());
                } else if let Some(query) = picker.current() {
                    // The field stays until the list comes back renamed: a refused name
                    // is still there to fix.
                    let id = query.id.clone();
                    picker.error = None;
                    return vec![Effect::RenameSavedQuery {
                        project_id,
                        id,
                        name,
                    }];
                }
            }
            _ => {
                input.handle_key(key);
            }
        }
        return Vec::new();
    }
    match key.code {
        KeyCode::Esc => picker.open = false,
        KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
        KeyCode::Down => {
            picker.selected += 1;
            picker.clamp();
        }
        KeyCode::Enter => return open_saved_query(model),
        KeyCode::F(2) => {
            if let Some(name) = picker.current().map(|query| query.name.clone()) {
                picker.renaming = Some(crate::widgets::text_input::TextInput::new(name));
                picker.error = None;
            }
        }
        KeyCode::Delete if picker.current().is_some() => {
            // Cancel holds the focus: Enter alone keeps the query.
            picker.deleting = Some(crate::widgets::form::FooterFocus::Cancel);
            picker.error = None;
        }
        _ => {
            if picker.search.handle_key(key) {
                picker.selected = 0;
                picker.error = None;
            }
        }
    }
    Vec::new()
}

/// Enter: the query opens in a new document of its connection, which connects if it is
/// not the live one.
fn open_saved_query(model: &mut Model) -> Vec<Effect> {
    let Some(query) = model.saved_queries.current().cloned() else {
        return Vec::new();
    };
    model.saved_queries.open = false;
    let title = crate::screens::document_name_prompt::normalize_document_name(
        &query.name.replace(['/', '\\'], "-"),
        "saved-query.sql",
    )
    .unwrap_or_else(|_| "saved-query.sql".into());
    let mut document =
        crate::model::EditorDocument::new_unique(title, None, Some(query.connection_id.clone()));
    document.sql = dexo_sql::SqlDocument::new(&query.sql);
    model.documents.push(document);
    let index = model.documents.len() - 1;
    let effects = activate_document(model, index);
    model.focus_active_document_tab();
    model.focus = Focus::Editor;
    effects
}

/// `f`: the rows the row under the cursor points at, or that point at it. The row is
/// read now, the keys are asked of the catalog, and the picker opens on them.
fn open_related_picker(model: &mut Model) -> Vec<Effect> {
    if !model.active_document().kind.is_table() {
        model
            .messages
            .warn("Related rows are a table's; open a table's rows first.".into());
        return Vec::new();
    }
    // Offline, the document's connection is dialled and `f` runs once it is up.
    if let Some(effects) = when_connected(model, Action::OpenRelatedPicker) {
        return effects;
    }
    let Some(session) = model.active_session else {
        model
            .messages
            .warn("Connect a session to follow foreign keys.".into());
        return Vec::new();
    };
    let Some((row, _)) = model.results.selection() else {
        model.messages.warn("Pick a row first.".into());
        return Vec::new();
    };
    let Some(values) = model.results.rows().get(row) else {
        return Vec::new();
    };
    model.data.related_row = model
        .results
        .columns()
        .iter()
        .zip(values)
        .map(|(column, value)| {
            let value = (!matches!(value, DbValue::Null)).then(|| value.clone());
            (column.name.clone(), value)
        })
        .collect();
    let table = model.data.target.clone();
    model.data.related_picker = Some(crate::screens::data::RelatedPicker {
        table: table.clone(),
        links: None,
        selected: 0,
    });
    vec![Effect::LoadForeignKeys {
        session,
        generation: model.session_generation,
        table,
    }]
}

/// Each key as a way out of `table`: `→` to the table a key of its own points at, `←`
/// from a table whose key points at it. A key of a table to itself goes both ways.
fn related_links(
    table: &dexo_driver_api::QualifiedName,
    keys: &[dexo_driver_api::ForeignKeyRef],
    dialect: dexo_sql::Dialect,
) -> Vec<crate::screens::data::RelatedLink> {
    // SQLite keeps a REFERENCES as written (`Customers`), and MySQL's names are as the
    // server folds them; Postgres's catalog names are exact.
    let name_eq = |a: &str, b: &str| match dialect {
        dexo_sql::Dialect::Postgres => a == b,
        dexo_sql::Dialect::Mysql | dexo_sql::Dialect::Sqlite | dexo_sql::Dialect::Duckdb => {
            a.eq_ignore_ascii_case(b)
        }
    };
    let same = |other: &dexo_driver_api::QualifiedName| {
        name_eq(other.object(), table.object())
            && match (
                other.schema().or(other.catalog()),
                table.schema().or(table.catalog()),
            ) {
                (Some(a), Some(b)) => name_eq(a, b),
                _ => true,
            }
    };
    let named = |other: &dexo_driver_api::QualifiedName| {
        let schema = other.schema().or(other.catalog());
        if schema == table.schema().or(table.catalog()) {
            other.object().to_string()
        } else {
            other.display_unquoted()
        }
    };
    let mut links = Vec::new();
    for key in keys {
        if same(&key.from) {
            links.push(crate::screens::data::RelatedLink {
                label: format!("→ {} ({})", named(&key.to), key.from_columns.join(", ")),
                key: dexo_app::data::ForeignKey {
                    local: key.from_columns.clone(),
                    referenced_table: key.to.clone(),
                    referenced: key.to_columns.clone(),
                },
            });
        }
        if same(&key.to) {
            links.push(crate::screens::data::RelatedLink {
                label: format!("← {} ({})", named(&key.from), key.from_columns.join(", ")),
                key: dexo_app::data::ForeignKey {
                    local: key.to_columns.clone(),
                    referenced_table: key.from.clone(),
                    referenced: key.from_columns.clone(),
                },
            });
        }
    }
    links
}

/// Enter in the picker: the chosen table opens in a document of its own, filtered to
/// the rows on the key's other end; `b` closes it and comes back.
fn open_related_link(model: &mut Model) -> Vec<Effect> {
    let Some(picker) = model.data.related_picker.as_ref() else {
        return Vec::new();
    };
    // Still asking the catalog: Enter waits with the picker.
    let Some(links) = picker.links.as_ref() else {
        return Vec::new();
    };
    let Some(link) = links.get(picker.selected).cloned() else {
        return Vec::new();
    };
    model.data.related_picker = None;
    open_related(model, link.key)
}

fn open_related(model: &mut Model, fk: dexo_app::data::ForeignKey) -> Vec<Effect> {
    let table = fk.referenced_table.display_unquoted();
    if fk.referenced.is_empty() || fk.referenced.len() != fk.local.len() {
        model.messages.warn(format!(
            "The key names no columns of {table}, which has no primary key to point at; there is nothing to follow."
        ));
        return Vec::new();
    }
    let Some(filter) = related_filter(&fk, &model.data.related_row) else {
        model.messages.warn(format!(
            "This row's {} is NULL, so it points at no row of {table}.",
            fk.local.join(", ")
        ));
        return Vec::new();
    };
    // Each hop is a document of its own, right after the one it came from: reusing an
    // open document of the table overwrote it -- the way back with it -- and kept that
    // document's own WHERE, which could hide the rows the key leads to.
    let origin = model.active_document;
    let mut document = crate::model::EditorDocument::new_table(
        fk.referenced_table.clone(),
        active_connection_uuid(model),
    );
    document.related_from = Some(model.documents[origin].id.clone());
    model.documents.insert(origin + 1, document);
    model.set_active_document(origin + 1);
    model.data.filter = Some(filter);
    model.focus_active_document_tab();
    model.focus = Focus::Results;
    load_table_document(model, origin + 1)
}

/// `b`: back to the document a related row was followed from, closing this hop's.
fn data_nav_back(model: &mut Model) -> Vec<Effect> {
    let Some(origin) = model.active_document().related_from.clone() else {
        model
            .messages
            .info("These rows were not opened from a related row; there is no way back.".into());
        return Vec::new();
    };
    if reload_would_orphan_edits(model) {
        return Vec::new();
    }
    let Some(index) = model
        .documents
        .iter()
        .position(|document| document.id == origin)
    else {
        model
            .messages
            .warn("The document these rows came from is closed.".into());
        return Vec::new();
    };
    let hop = model.active_document;
    model.set_active_document(index);
    let effects = remove_document(model, hop);
    model.focus = Focus::Results;
    effects
}

/// The grid's copy, from the palette, a key or the Enter menu alike: "Copy as ..." takes
/// the cursor's row (or the rows selected) whole, and "Copy cell" the value alone.
fn copy_grid(model: &mut Model, format: dexo_app::data::CopyFormat) -> Vec<Effect> {
    use dexo_app::data::CopyFormat;
    if format == CopyFormat::Value {
        if let Some((row, col)) = model.results.selection() {
            model.results.select_cell(row, col);
        }
    } else {
        model.results.widen_selection_to_rows();
    }
    // The rows of an opened table insert back into it; a query's have no table to name.
    let table = model.active_document().kind.is_table().then(|| {
        let target = &model.data.target;
        match target.schema().or(target.catalog()) {
            Some(scope) => format!("{scope}.{}", target.object()),
            None => target.object().to_string(),
        }
    });
    match model
        .results
        .copy_of(format, model.data.dialect, table.as_deref())
    {
        Ok(copied) if copied.text.len() > 8 * 1024 * 1024 => {
            model
                .messages
                .warn("selection too large for clipboard; export to a file".into());
            Vec::new()
        }
        Ok(copied) => {
            model.data.copy_note = Some(copy_note(format, copied.rows, copied.columns));
            vec![Effect::CopyToClipboard { text: copied.text }]
        }
        Err(message) => {
            model.messages.error(message);
            Vec::new()
        }
    }
}

/// What the toast says went to the clipboard: `copied the cell`, `copied 3 rows, 2
/// columns as CSV`.
fn copy_note(format: dexo_app::data::CopyFormat, rows: usize, columns: usize) -> String {
    use dexo_app::data::CopyFormat;
    let name = match format {
        CopyFormat::Value => None,
        CopyFormat::Text => Some("text"),
        CopyFormat::Csv => Some("CSV"),
        CopyFormat::Tsv => Some("TSV"),
        CopyFormat::Json => Some("JSON"),
        CopyFormat::Markdown => Some("Markdown"),
        CopyFormat::Sql => Some("SQL"),
    };
    let plural = |count: usize, noun: &str| {
        if count == 1 {
            format!("1 {noun}")
        } else {
            format!("{count} {noun}s")
        }
    };
    let what = if (rows, columns) == (1, 1) && name.is_none() {
        "the cell".to_string()
    } else {
        format!("{}, {}", plural(rows, "row"), plural(columns, "column"))
    };
    match name {
        Some(name) => format!("copied {what} as {name}"),
        None => format!("copied {what}"),
    }
}

fn apply_changes(model: &mut Model) -> Vec<Effect> {
    if model.connection.read_only {
        model.messages.warn("connection is read-only".into());
        return Vec::new();
    }
    // Production is read from the connection: the palette's Apply Changes arrives with
    // no review, and shows the changes first. Then the connection's name is typed, as
    // for a statement run from the editor; a click on the review used to be enough.
    if on_production(model) {
        if model.data.review.is_none() {
            model.data.environment =
                dexo_app::Environment::parse_strict(&model.connection.environment);
            model.data.open_review();
            return Vec::new();
        }
        let count = model.data.changes.pending().len();
        let what = vec![format!(
            "Apply {count} {} to {}.",
            if count == 1 { "change" } else { "changes" },
            model.data.target.display_unquoted()
        )];
        if !production_cleared(model, what, Action::ApplyChanges) {
            return Vec::new();
        }
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

/// The schema form answers like every other dialog -- Esc cancels, the arrows walk to
/// its buttons -- once the walk has passed its last field. It used to take Tab, Enter
/// and Esc and nothing else: nothing typed reached a field, and it had no buttons.
fn schema_form_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterFocus, FooterKey, confirm_key, footer_key};
    let editor = &mut model.schema_editor;
    let last = editor.fields.len().saturating_sub(1);
    // Opened on raw SQL, Run runs that SQL and nothing else: the fields are not shown,
    // and the keys walk the two buttons only. They used to take typing Run ignored.
    if editor.is_raw() {
        return match confirm_key(&mut editor.footer, &key) {
            FooterKey::Submit => submit_schema_form(model),
            FooterKey::Cancel => {
                model.schema_editor.open = false;
                Vec::new()
            }
            FooterKey::Moved | FooterKey::Pass => Vec::new(),
        };
    }
    if editor.footer == FooterFocus::Input {
        match key.code {
            KeyCode::Tab | KeyCode::Down if editor.focus < last => {
                editor.focus_next();
                return Vec::new();
            }
            KeyCode::BackTab | KeyCode::Up if editor.focus > 0 => {
                editor.focus_prev();
                return Vec::new();
            }
            _ if editor.edit_focused(key) => return Vec::new(),
            _ => {}
        }
    }
    let before = editor.footer;
    match footer_key(&mut editor.footer, &key) {
        FooterKey::Submit => submit_schema_form(model),
        FooterKey::Cancel => {
            model.schema_editor.open = false;
            Vec::new()
        }
        FooterKey::Moved => {
            // Walking back into the fields lands on the end it came in from.
            let editor = &mut model.schema_editor;
            if editor.footer == FooterFocus::Input {
                editor.focus = if before == FooterFocus::Cancel {
                    0
                } else {
                    last
                };
            }
            Vec::new()
        }
        FooterKey::Pass => Vec::new(),
    }
}

/// Previews the DDL the fields make, closing the form for the preview to take the keys;
/// a form opened on raw SQL runs it the way the editor runs a document, through the
/// connection's guard.
fn submit_schema_form(model: &mut Model) -> Vec<Effect> {
    if !model.schema_editor.raw_sql.is_empty() {
        model.schema_editor.open = false;
        return update(model, Action::ExecuteDocument);
    }
    let effects = update(model, Action::OpenDdlPreview);
    if model.schema_editor.errors.is_empty() {
        model.schema_editor.open = false;
    }
    effects
}

fn open_ddl_preview(model: &mut Model) -> Vec<Effect> {
    if !model.schema_editor.validate() {
        return Vec::new();
    }
    let Ok(change) = model.schema_editor.to_change() else {
        return Vec::new();
    };
    model.schema_editor.pending = Some((
        crate::screens::schema_editor::PreviewOrigin::Form,
        change.clone(),
    ));
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

/// The DDL preview answers like every dialog: Esc cancels, the arrows walk the name to
/// type and the two buttons, Enter applies unless Cancel has the focus. With no name to
/// type, the walk stays on the buttons.
fn ddl_preview_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterFocus, FooterKey, confirm_key, footer_key};
    let Some(preview) = model.schema_editor.preview.as_mut() else {
        return Vec::new();
    };
    // A statement longer than the dialog is read a page at a time.
    let page = i32::from((model.height / 3).max(3));
    let paged = match key.code {
        KeyCode::PageDown => Some(page),
        KeyCode::PageUp => Some(-page),
        _ => None,
    };
    if let Some(delta) = paged {
        preview.scroll = model.hits.scroll(
            crate::mouse::ScrollArea::DdlPreview,
            preview.scroll.min(usize::from(u16::MAX)) as u16,
            delta,
        ) as usize;
        return Vec::new();
    }
    let outcome = if preview.needs_typing() {
        footer_key(&mut preview.footer, &key)
    } else {
        confirm_key(&mut preview.footer, &key)
    };
    match outcome {
        FooterKey::Submit => apply_ddl(model),
        FooterKey::Cancel => cancel_ddl_preview(model),
        FooterKey::Moved => Vec::new(),
        FooterKey::Pass => {
            if preview.needs_typing()
                && preview.footer == FooterFocus::Input
                && preview.typed.handle_key(key)
            {
                preview.error = None;
                model.schema_editor.confirm_typed();
            }
            Vec::new()
        }
    }
}

/// Cancel in the preview goes back to what asked for it: the form, with its fields as
/// they were, to be changed -- it closed the form with the preview, and the way back was
/// the palette -- or the Security panel underneath.
fn cancel_ddl_preview(model: &mut Model) -> Vec<Effect> {
    use crate::screens::schema_editor::PreviewOrigin;
    let origin = model
        .schema_editor
        .preview
        .as_ref()
        .map(|preview| preview.origin);
    model.schema_editor.preview = None;
    if origin == Some(PreviewOrigin::Form) {
        model.schema_editor.open = true;
        model.schema_editor.footer = crate::widgets::form::FooterFocus::Input;
    }
    Vec::new()
}

/// Reads again the place of the explorer a schema change touched: the schema it created
/// a table in, or the database of a MySQL table. The tree kept showing what was there
/// before until `r` was pressed on the schema.
fn refresh_after_schema_change(
    model: &mut Model,
    target: &dexo_driver_api::QualifiedName,
) -> Vec<Effect> {
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    let wanted = target.schema().or(target.catalog()).map(str::to_string);
    let Some(wanted) = wanted else {
        return Vec::new();
    };
    let id = model.explorer.find_container(&wanted);
    let Some(id) = id else {
        return Vec::new();
    };
    let operation = crate::runtime::OperationId::new();
    if model.explorer.expand_with(&id, operation) {
        let _ = session;
        return catalog_load_effect(model, Some(id), operation, false);
    }
    Vec::new()
}

fn apply_ddl(model: &mut Model) -> Vec<Effect> {
    if model.connection.read_only {
        model.messages.warn("connection is read-only".into());
        return Vec::new();
    }
    let Some(preview) = &mut model.schema_editor.preview else {
        return Vec::new();
    };
    if preview.needs_typing() && !preview.confirmed {
        preview.error = Some("The name does not match; nothing was applied.".into());
        return Vec::new();
    }
    let mut what = vec![format!("Apply to {}:", preview.target)];
    what.extend(preview.sql.lines().take(3).map(|line| format!("  {line}")));
    let typed = preview.typed.as_str().to_string();
    // What was previewed is what is applied: a grant from the Security panel used to be
    // answered with the Schema form's table, and "ddl RolledBack".
    let origin = preview.origin;
    let Some(change) = preview.change.clone() else {
        return Vec::new();
    };
    if !production_cleared(model, what, Action::ApplyDdl) {
        return Vec::new();
    }
    model.schema_editor.applying = Some(origin);
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

/// The first `query-N.sql` no open document has. Counting the documents gave a name
/// already open as soon as one had been closed or renamed.
fn suggested_document_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("query-{n}.sql"))
        .find(|name| {
            !model
                .documents
                .iter()
                .any(|document| document.title.eq_ignore_ascii_case(name))
        })
        .unwrap_or_else(|| "query.sql".into())
}

fn open_new_document_prompt(model: &mut Model) {
    let default_name = suggested_document_name(model);
    let connection = new_document_connection(model);
    model.document_name_prompt =
        crate::screens::document_name_prompt::DocumentNamePrompt::open_create(
            default_name,
            connection,
        );
}

/// The connection a new document is for, as `(id, name)`. From the explorer it is the
/// one under the cursor -- the one in view -- and the connection last made active
/// otherwise: Ctrl+N on `pg-dev` made a document that ran on SQLite.
fn new_document_connection(model: &Model) -> Option<(String, String)> {
    let picked = (model.focus == Focus::Explorer)
        .then(|| model.explorer.selected_connection_name())
        .flatten();
    let name = picked.unwrap_or(model.connection.name.as_str());
    model
        .connections
        .profiles
        .iter()
        .find(|row| row.profile.name == name)
        .map(|row| (row.profile.id.0.to_string(), row.profile.name.clone()))
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
    // Nothing typed is not a request for the suggestion: the field was cleared on
    // purpose, and the dialog used to close as if it had been answered.
    if model.document_name_prompt.name.trim().is_empty() {
        model.document_name_prompt.error = Some("name cannot be empty".into());
        return Vec::new();
    }
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
            let connection_id = model
                .document_name_prompt
                .connection
                .as_ref()
                .map(|(id, _)| id.clone());
            model
                .documents
                .push(crate::model::EditorDocument::new_unique(
                    name,
                    None,
                    connection_id,
                ));
            let index = model.documents.len() - 1;
            model.active_document = index;
            model.focus_active_document_tab();
            model.focus = Focus::Editor;
            // The session follows the document, as it does for a tab picked from the
            // strip: the header and the status bar named the connection left behind.
            return activate_document(model, index);
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

/// The output pane of the document with this id: the one on screen when it is that
/// document's, or the one parked with the document otherwise.
fn results_of_document<'a>(
    model: &'a mut Model,
    document: &str,
) -> Option<&'a mut crate::model::ResultsState> {
    if model.results_owner.as_deref() == Some(document) {
        return Some(&mut model.results);
    }
    model
        .documents
        .iter_mut()
        .find(|candidate| candidate.id == document)
        .map(|candidate| &mut candidate.results)
}

/// EXPLAIN ANALYZE runs the statement under the cursor, so on a read-only connection
/// a statement that is not a read is refused here, before anything is sent, with the
/// editor's own words. It used to reach the server, which refused it.
fn analyze_refused(model: &mut Model) -> bool {
    if !model.connection.read_only {
        return false;
    }
    let document = model.active_document();
    let text = document.text();
    let cursor = text
        .chars()
        .take(document.cursor())
        .map(char::len_utf8)
        .sum();
    let dialect = crate::screens::editor::editor_dialect(model);
    let Some(sql) = crate::runtime::explain_manager::statement_sql(&text, cursor, dialect) else {
        return false;
    };
    if dexo_sql::is_read(&sql, dialect) {
        return false;
    }
    let first = sql.lines().next().unwrap_or_default().to_string();
    model.messages.error(format!(
        "Not run: {} is read-only, and EXPLAIN ANALYZE would run a statement that is not a read: {first}",
        model.connection.name,
    ));
    true
}

/// EXPLAIN is an operation like a run: it holds the slot Ctrl+F2 cancels, and it waits
/// for one already running instead of racing it on the same session. It explains
/// `statement` when given, else the statement under the cursor.
fn explain_effect(
    model: &mut Model,
    analyze: bool,
    statement: Option<String>,
    indexes: Vec<String>,
) -> Vec<Effect> {
    crate::screens::editor::close_completion(model);
    let Some(session) = model.active_session else {
        return Vec::new();
    };
    if analyze && analyze_refused(model) {
        return Vec::new();
    }
    if model.active_operation.is_some() {
        model
            .messages
            .warn("a statement is still running; cancel it with Ctrl+F2 first".into());
        return Vec::new();
    }
    let document = model.active_document();
    let text = document.text();
    let under_cursor = dexo_sql::statement_at_in(
        &text,
        document.byte_cursor(),
        crate::screens::editor::editor_dialect(model),
    );
    if statement.is_none()
        && under_cursor.is_some_and(|span| dexo_sql::is_backslash_command(&text[span.byte_range]))
    {
        model.messages.warn(
            "A backslash command is answered by Dexo from the catalog; the server has no plan for it."
                .into(),
        );
        return Vec::new();
    }
    let operation = crate::runtime::OperationId::new();
    model.active_operation = Some(operation);
    model.results.view = crate::model::ResultsView::Explain;
    model.results.explain_scroll = 0;
    let document = model.active_document();
    let (sql, cursor) = match statement {
        Some(statement) => (statement, 0),
        None => {
            let sql = document.text();
            let cursor = sql
                .chars()
                .take(document.cursor())
                .map(char::len_utf8)
                .sum();
            (sql, cursor)
        }
    };
    vec![Effect::RunExplain {
        sql,
        cursor,
        dialect: crate::screens::editor::editor_dialect(model),
        analyze,
        indexes,
        session,
        document: document.id.clone(),
        operation,
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
    // A table's rows staged for the database are as unsaved as typed text: closing the tab
    // would drop them without a word.
    let staged = model.active_document().kind.is_table() && model.data.has_pending_edits();
    if model.active_document().is_dirty() || staged {
        let document = model.active_document();
        model.close_prompt = Some(crate::model::ClosePrompt {
            document: document.id.clone(),
            title: document.title.clone(),
            choice: crate::model::CloseChoice::Save,
        });
        return Vec::new();
    }
    remove_document(model, model.active_document)
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
            let mut effects = remove_document(model, index);
            effects.push(Effect::DiscardRecovery {
                document: prompt.document,
            });
            effects
        }
        // The changes staged in a table go to the database through the review, which asks
        // what a write there asks, and the tab stays until they are in.
        CloseChoice::Save if model.documents[index].kind.is_table() => {
            model.set_active_document(index);
            update(model, Action::OpenReview)
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

fn handle_run_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    let Some(prompt) = model.run_prompt.as_mut() else {
        return Vec::new();
    };
    match footer_key(&mut prompt.footer, &key) {
        FooterKey::Submit => submit_run_prompt(model),
        FooterKey::Cancel => {
            model.run_prompt = None;
            Vec::new()
        }
        FooterKey::Moved => {
            // Nothing to type: the walk skips the input stop it would land on.
            if prompt.expected.is_none() && prompt.footer == FooterFocus::Input {
                prompt.footer = if matches!(key.code, KeyCode::BackTab | KeyCode::Up) {
                    FooterFocus::Cancel
                } else {
                    FooterFocus::Submit
                };
            }
            Vec::new()
        }
        FooterKey::Pass => {
            if prompt.expected.is_some() && prompt.footer == FooterFocus::Input {
                let _ = prompt.typed.handle_key(key);
                prompt.error = None;
            }
            Vec::new()
        }
    }
}

fn submit_run_prompt(model: &mut Model) -> Vec<Effect> {
    let Some(prompt) = model.run_prompt.as_mut() else {
        return Vec::new();
    };
    if !prompt.accepted() {
        prompt.error = Some("The name does not match; nothing was run.".into());
        return Vec::new();
    }
    // A closing session can switch the editor to another connection without a
    // connection change; what was confirmed for one never runs on another.
    if prompt.connection != model.connection.name || prompt.session != model.active_session {
        model.run_prompt = None;
        model
            .messages
            .warn("The connection changed under the dialog; nothing was run.".into());
        return Vec::new();
    }
    let statements = std::mem::take(&mut prompt.statements);
    model.run_prompt = None;
    launch_script(model, statements)
}

/// Run and Cancel, nothing to type.
fn handle_quit_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, confirm_key};
    let Some(focus) = model.quit_prompt.as_mut() else {
        return Vec::new();
    };
    match confirm_key(focus, &key) {
        FooterKey::Submit => update(model, Action::Quit),
        FooterKey::Cancel => {
            model.quit_prompt = None;
            Vec::new()
        }
        FooterKey::Moved | FooterKey::Pass => Vec::new(),
    }
}

/// What quitting would throw away: each open transaction, rolled back when its session
/// closes, and the grid edits not yet applied, in every document.
pub(crate) fn quit_losses(model: &Model) -> Vec<String> {
    let mut losses: Vec<String> = model
        .connections
        .sessions
        .iter()
        .filter(|row| row.transaction != dexo_driver_api::TransactionState::Idle)
        .map(|row| {
            format!(
                "A transaction is open on {}: it is rolled back.",
                row.connection
            )
        })
        .collect();
    let parked: usize = model
        .documents
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != model.active_document)
        .map(|(_, document)| document.browse.changes.pending().len())
        .sum();
    if model.active_query.is_some() && model.active_operation.is_some() {
        losses.push(format!(
            "A query is still running on {}: quitting cancels it.",
            model.connection.name
        ));
    }
    let edits = model.data.changes.pending().len() + parked;
    if edits > 0 {
        losses.push(format!(
            "{edits} grid {} not applied: {} lost.",
            if edits == 1 { "edit is" } else { "edits are" },
            if edits == 1 { "it is" } else { "they are" },
        ));
    }
    losses
}

fn on_production(model: &Model) -> bool {
    dexo_app::Environment::parse_strict(&model.connection.environment)
        == dexo_app::Environment::Production
}

/// On production, a write the editor's guard does not see -- grid edits, DDL from the
/// schema form, an import, a restore, EXPLAIN ANALYZE of a write -- waits for the
/// connection's name, as a statement run from the editor does. True when `then` may go
/// ahead now: off production, or in the dispatch right after the name was typed.
/// Otherwise the prompt opens, and `then` is dispatched again once the name is typed.
fn production_cleared(model: &mut Model, what: Vec<String>, then: Action) -> bool {
    if !on_production(model) || std::mem::take(&mut model.production_cleared) {
        return true;
    }
    model.production_prompt = Some(crate::screens::production_prompt::ProductionPrompt::new(
        model.connection.name.clone(),
        model.active_session,
        what,
        then,
    ));
    false
}

fn handle_production_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    let Some(prompt) = model.production_prompt.as_mut() else {
        return Vec::new();
    };
    match footer_key(&mut prompt.footer, &key) {
        FooterKey::Submit => submit_production_prompt(model),
        FooterKey::Cancel => {
            model.production_prompt = None;
            Vec::new()
        }
        FooterKey::Moved => Vec::new(),
        FooterKey::Pass => {
            if prompt.footer == FooterFocus::Input {
                let _ = prompt.typed.handle_key(key);
                prompt.error = None;
            }
            Vec::new()
        }
    }
}

fn submit_production_prompt(model: &mut Model) -> Vec<Effect> {
    let Some(prompt) = model.production_prompt.as_mut() else {
        return Vec::new();
    };
    if !prompt.accepted() {
        prompt.error = Some("The name does not match; nothing was done.".into());
        return Vec::new();
    }
    let Some(prompt) = model.production_prompt.take() else {
        return Vec::new();
    };
    // What was confirmed for one connection is never done on another.
    if prompt.connection != model.connection.name || prompt.session != model.active_session {
        model
            .messages
            .warn("The connection changed under the dialog; nothing was done.".into());
        return Vec::new();
    }
    model.production_cleared = true;
    let effects = update(model, *prompt.then);
    // One write per name typed: never left over for the next one.
    model.production_cleared = false;
    effects
}

/// The statement EXPLAIN ANALYZE would run, when it is not a read.
fn analyzed_write(model: &Model) -> Option<String> {
    let document = model.active_document();
    let text = document.text();
    let cursor = text
        .chars()
        .take(document.cursor())
        .map(char::len_utf8)
        .sum();
    let dialect = crate::screens::editor::editor_dialect(model);
    crate::runtime::explain_manager::statement_sql(&text, cursor, dialect)
        .filter(|sql| !dexo_sql::is_read(sql, dialect))
}

fn handle_explain_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, confirm_key};
    let Some(focus) = model.explain_prompt.as_mut() else {
        return Vec::new();
    };
    match confirm_key(focus, &key) {
        FooterKey::Submit => update(model, Action::RunExplainAnalyze),
        FooterKey::Cancel => {
            model.explain_prompt = None;
            Vec::new()
        }
        FooterKey::Moved | FooterKey::Pass => Vec::new(),
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

fn remove_document(model: &mut Model, index: usize) -> Vec<Effect> {
    if index >= model.documents.len() {
        return Vec::new();
    }
    let was_active = index == model.active_document;
    // A count still running for it is stopped on the server, not left counting.
    let mut effects = if index == model.active_document {
        drop_count(model)
    } else {
        match model.documents[index]
            .browse
            .count
            .take()
            .map(|count| count.state)
        {
            Some(crate::screens::data::CountState::Running(operation)) => {
                vec![Effect::CancelCount { operation }]
            }
            _ => Vec::new(),
        }
    };
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
    // The document that is on screen now brings its connection with it, as when it is
    // picked from the strip; the header and the status bar kept the closed one's.
    if was_active && !model.active_document().kind.is_placeholder() {
        let next = model.active_document;
        effects.extend(activate_document(model, next));
    }
    effects
}

/// Steps through Dexo's own theme, the presets and the user's files; each applies the
/// moment it is shown, so the app itself is the preview.
fn cycle_theme(model: &mut Model, delta: i32) -> Vec<Effect> {
    let choices = &model.settings.themes;
    if !choices.is_empty() {
        let current = choices
            .iter()
            .position(|(key, _)| *key == model.settings.theme)
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(choices.len() as i32) as usize;
        model.settings.theme = choices[next].0.clone();
    }
    rebuild_theme(model);
    Vec::new()
}

/// Mode and accent are the Dexo theme's: changing either goes back to it.
fn cycle_mode(model: &mut Model, delta: i32) -> Vec<Effect> {
    let next = crate::theme::Mode::from_key(&model.settings.mode).step(delta);
    model.settings.mode = next.as_key().into();
    model.settings.theme = crate::theme::DEXO_THEME.into();
    rebuild_theme(model);
    Vec::new()
}

fn cycle_accent(model: &mut Model, delta: i32) -> Vec<Effect> {
    model.settings.accent = crate::theme::step_accent(&model.settings.accent, delta).into();
    model.settings.theme = crate::theme::DEXO_THEME.into();
    rebuild_theme(model);
    Vec::new()
}

/// The theme chosen: a preset, a file of the user's, or Dexo's own composed from the
/// surface and the primary color.
fn rebuild_theme(model: &mut Model) {
    model.theme = crate::theme::resolve(
        &model.settings.theme,
        crate::theme::Mode::from_key(&model.settings.mode),
        &model.settings.accent,
        &model.user_themes,
    );
    persist_settings(model);
}

fn cycle_keymap(model: &mut Model, delta: i32) -> Vec<Effect> {
    let next = crate::keymap::step_profile(&model.keymap.name, delta);
    set_keymap(model, next);
    model.settings.keymap = model.keymap.name.clone();
    persist_settings(model);
    Vec::new()
}

/// The profile `name` with the user's `keymap.toml` over it, saying what was wrong with
/// the file when it could not be used.
fn set_keymap(model: &mut Model, name: &str) {
    let Ok(paths) = dexo_storage::AppPaths::discover() else {
        model.keymap = crate::keymap::Keymap::named(name);
        return;
    };
    let (keymap, problem) = crate::keymap::load(name, &paths.data_dir);
    model.keymap = keymap;
    if let Some(problem) = problem {
        model.messages.warn(problem);
    }
}

/// `delta` is the arrow direction; the two-value rows ignore it because they toggle.
fn step_focused_setting(model: &mut Model, delta: i32) -> Vec<Effect> {
    match model.settings.focus {
        0 => cycle_theme(model, delta),
        1 => cycle_mode(model, delta),
        2 => cycle_accent(model, delta),
        3 => cycle_keymap(model, delta),
        4 => update(model, Action::ToggleMouse),
        5 => update(model, Action::ToggleAnimation),
        6 => update(model, Action::ToggleUnicode),
        7 => update(model, Action::ToggleUpdateCheck),
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
    set_keymap(model, "default");
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
        color_theme: model.settings.theme.clone(),
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
    set_keymap(model, &manager.active.keymap.profile);
    let mode = crate::theme::mode_from_settings(manager.active.mode);
    model.settings.mode = mode.as_key().into();
    model.settings.accent = manager.active.accent.clone();
    model.settings.completion_trigger = manager.active.completion_trigger;
    model.settings.updates = manager.active.update_check;
    model.settings.theme = manager.active.color_theme.clone();
    load_user_themes(model, &paths.data_dir);
    sync_settings_screen(model);
}

/// The user's theme files read again, each one that does not parse said by file and
/// line once; the theme in use painted from what its file says now, or Dexo's own when
/// its file is gone.
fn load_user_themes(model: &mut Model, data_dir: &std::path::Path) {
    let (themes, problems) = crate::theme::user_themes(data_dir);
    for problem in &problems {
        if !model.theme_problems.contains(problem) {
            model
                .messages
                .warn(format!("{problem}; that theme is left out"));
        }
    }
    model.theme_problems = problems;
    model.settings.themes = crate::theme::choices(&themes);
    model.user_themes = themes;
    if !model
        .settings
        .themes
        .iter()
        .any(|(key, _)| *key == model.settings.theme)
    {
        if model.settings.theme != crate::theme::DEXO_THEME {
            model.messages.warn(format!(
                "There is no theme {}; Dexo's own is used.",
                model.settings.theme.trim_start_matches("file:")
            ));
        }
        model.settings.theme = crate::theme::DEXO_THEME.into();
    }
    model.theme = crate::theme::resolve(
        &model.settings.theme,
        crate::theme::Mode::from_key(&model.settings.mode),
        &model.settings.accent,
        &model.user_themes,
    );
}

fn run_transfer(model: &mut Model) -> Vec<Effect> {
    // Import and Restore write into the database, Restore through a native tool whose
    // connection never gets the session's read-only setting, so Dexo refuses first.
    if model.connection.read_only
        && matches!(
            model.transfer.mode,
            crate::screens::transfer::TransferMode::Import
                | crate::screens::transfer::TransferMode::Restore
        )
    {
        model.transfer.error =
            Some("The connection is read-only; import and restore write into it.".into());
        return Vec::new();
    }
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
    let what = match model.transfer.mode {
        crate::screens::transfer::TransferMode::Import => Some(format!(
            "Import {} into {}.",
            path.display(),
            model.data.target.display_unquoted()
        )),
        crate::screens::transfer::TransferMode::Restore => Some(format!(
            "Restore {} into the database of {}.",
            path.display(),
            model.connection.name
        )),
        _ => None,
    };
    if let Some(what) = what
        && !production_cleared(model, vec![what], Action::SubmitTransfer)
    {
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
                dialect: model.data.dialect,
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

/// Compare Schema opens on the connections that are open and the snapshots saved, to pick
/// two from. It took both sides from the one selected connection.
fn open_schema_diff(model: &mut Model) -> Vec<Effect> {
    use crate::screens::schema_diff::{DiffOption, DiffOptionKind};
    let mut options: Vec<DiffOption> = model
        .connections
        .profiles
        .iter()
        .filter_map(|row| {
            let session = model.connections.session_for(&row.profile.name)?;
            Some(DiffOption {
                label: format!("{}  (connected)", row.profile.name),
                name: row.profile.name.clone(),
                kind: DiffOptionKind::Live(session.id),
                driver: row.profile.driver.clone(),
            })
        })
        .collect();
    options.push(DiffOption {
        label: "a snapshot file...".into(),
        name: String::new(),
        kind: DiffOptionKind::File,
        driver: String::new(),
    });
    let active = (!model.connection.name.is_empty()).then(|| model.connection.name.clone());
    model.schema_diff.open_picker(options, active.as_deref());
    vec![Effect::LoadSchemaSources]
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
    model.schema_editor.pending = Some((
        crate::screens::schema_editor::PreviewOrigin::Security,
        change.clone(),
    ));
    vec![Effect::PreviewDdl {
        change,
        session,
        generation: model.session_generation,
    }]
}

/// An import reads data, not SQL: an SQL file is a script, run in the editor.
fn next_transfer_format(current: &str, import: bool) -> String {
    match current {
        "csv" => "tsv",
        "tsv" => "json",
        "json" => "jsonl",
        "jsonl" if !import => "sql",
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
        use crate::widgets::form::{FooterKey, confirm_key};
        return match confirm_key(&mut model.editor.history_footer, &key) {
            FooterKey::Submit => confirm_clear_history(model),
            FooterKey::Cancel => {
                model.editor.history_confirm_clear = false;
                model.editor.history_open = false;
                Vec::new()
            }
            FooterKey::Moved | FooterKey::Pass => Vec::new(),
        };
    }
    if key.code == KeyCode::Enter {
        return update(model, Action::HistoryPick);
    }
    crate::screens::editor::handle_history_key(model, key);
    Vec::new()
}

fn handle_admin_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterKey, footer_key};
    if let Some(prompt) = model.admin.terminate.as_mut() {
        return match footer_key(&mut prompt.footer, &key) {
            FooterKey::Submit => submit_terminate(model),
            FooterKey::Cancel => {
                model.admin.terminate = None;
                Vec::new()
            }
            FooterKey::Moved => Vec::new(),
            FooterKey::Pass => {
                if prompt.footer == crate::widgets::form::FooterFocus::Input {
                    let _ = prompt.typed.handle_key(key);
                    prompt.error = None;
                }
                Vec::new()
            }
        };
    }
    match key.code {
        KeyCode::Esc => {
            model.admin.open = false;
            Vec::new()
        }
        KeyCode::Up | KeyCode::Char('k') => {
            model.admin.move_selection(false);
            Vec::new()
        }
        KeyCode::Down | KeyCode::Char('j') => {
            model.admin.move_selection(true);
            Vec::new()
        }
        KeyCode::Char('r') => load_admin_sessions(model),
        KeyCode::Char('t') => open_terminate(model),
        _ => Vec::new(),
    }
}

fn load_admin_sessions(model: &Model) -> Vec<Effect> {
    model
        .active_session
        .map(|session| Effect::LoadAdminSessions {
            session,
            generation: model.session_generation,
        })
        .into_iter()
        .collect()
}

/// Ending a session is a write on the server: the connection's policy is asked first,
/// then the picked session's id has to be typed.
fn open_terminate(model: &mut Model) -> Vec<Effect> {
    let Some(session) = model.admin.picked().cloned() else {
        return Vec::new();
    };
    let action = dexo_driver_api::AdminAction::TerminateSession {
        session_id: session.id.clone(),
    };
    let policy = dexo_app::admin_service::AdminPolicy {
        production: dexo_app::Environment::parse_strict(&model.connection.environment)
            == dexo_app::Environment::Production,
        read_only: model.connection.read_only,
    };
    let decision = dexo_app::admin_service::evaluate(&action, "", &policy);
    if !decision.allowed {
        model.admin.last_error = Some(format!(
            "Not terminated: {} is read-only.",
            model.connection.name
        ));
        return Vec::new();
    }
    let mut prompt = crate::screens::admin::TerminatePrompt::new(session);
    prompt.connection = model.connection.name.clone();
    model.admin.terminate = Some(prompt);
    Vec::new()
}

fn submit_terminate(model: &mut Model) -> Vec<Effect> {
    let Some(prompt) = model.admin.terminate.as_mut() else {
        return Vec::new();
    };
    if !prompt.accepted() {
        prompt.error = Some("The id does not match; nothing was done.".into());
        return Vec::new();
    }
    // What was confirmed for one connection is never sent to another.
    let target = prompt.session.id.clone();
    let connection = prompt.connection.clone();
    model.admin.terminate = None;
    match model.active_session {
        Some(session) if connection == model.connection.name => {
            vec![Effect::AdminTerminate { session, target }]
        }
        _ => {
            model.admin.last_error =
                Some("The connection changed under the dialog; nothing was done.".into());
            Vec::new()
        }
    }
}

/// File rows the picker shows: the same count its layout draws.
fn file_picker_rows(model: &Model) -> usize {
    model.file_picker.browser_rows(
        model.file_picker_mode,
        crate::screens::file_picker::inner_rows(model.height),
    )
}

fn handle_file_picker_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::file_picker::FilePickerFocus;
    let rows = file_picker_rows(model);
    if let Some(confirm) = model.file_picker.confirm.as_mut() {
        use crate::widgets::form::{FooterKey, confirm_key};
        return match confirm_key(&mut confirm.focus, &key) {
            FooterKey::Submit => replace_file(model),
            FooterKey::Cancel => {
                model.file_picker.confirm = None;
                Vec::new()
            }
            FooterKey::Moved | FooterKey::Pass => Vec::new(),
        };
    }
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
        KeyCode::PageDown => {
            model.file_picker.page(1, rows);
            Vec::new()
        }
        KeyCode::PageUp => {
            model.file_picker.page(-1, rows);
            Vec::new()
        }
        KeyCode::Home | KeyCode::End if model.file_picker.focus == FilePickerFocus::List => {
            model
                .file_picker
                .jump_to_end(key.code == KeyCode::End, rows);
            Vec::new()
        }
        // Every key the name edits with, Ctrl+A and Ctrl+W among them: only the arrows,
        // Backspace, Delete and plain letters reached it.
        _ if model.file_picker.focus == FilePickerFocus::Name
            && crate::widgets::text_input::TextInput::owns(&key) =>
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

/// A write to a file that is already there waits for a yes: the picker used to replace
/// whatever was in it, a database among the files it offered.
fn file_picker_submit(model: &mut Model) -> Vec<Effect> {
    let Some(path) = model.file_picker.chosen_path() else {
        model.file_picker.error = Some("choose a file or type a name".into());
        return Vec::new();
    };
    if file_picker_writes(model) {
        if path.is_dir() {
            model.file_picker.error = Some("that is a folder; type a file name".into());
            return Vec::new();
        }
        if path.exists() {
            model.file_picker.confirm = Some(crate::screens::file_picker::OverwriteConfirm {
                path,
                focus: crate::widgets::form::FooterFocus::Cancel,
            });
            return Vec::new();
        }
    }
    file_picker_accept(model, path)
}

/// Whether the picked path is written to rather than read: Open, Import and Restore
/// read the file they are given.
fn file_picker_writes(model: &Model) -> bool {
    use crate::screens::file_picker::FilePickerMode;
    match model.file_picker_mode {
        FilePickerMode::Save | FilePickerMode::Diagnostics | FilePickerMode::ConfigExport => true,
        FilePickerMode::Transfer => matches!(
            model.transfer.mode,
            crate::screens::transfer::TransferMode::Export
                | crate::screens::transfer::TransferMode::Backup
        ),
        FilePickerMode::Open | FilePickerMode::ConfigImport => false,
    }
}

fn replace_file(model: &mut Model) -> Vec<Effect> {
    match model.file_picker.confirm.take() {
        Some(confirm) => file_picker_accept(model, confirm.path),
        None => Vec::new(),
    }
}

fn file_picker_accept(model: &mut Model, path: std::path::PathBuf) -> Vec<Effect> {
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
            model.transfer.path.set_text(path.display().to_string());
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
            model.projects.closing_sessions = model.connections.sessions.len();
            model.projects.dirty_choice = (switch.stage
                == crate::runtime::project_manager::ProjectSwitchStage::ConfirmDirty)
                .then_some(crate::model::CloseChoice::Save);
            model.projects.pending = Some(switch.clone());
            crate::runtime::project_manager::advance(model, &switch)
        }
    }
}

/// The titles of the open documents with changes no file holds.
pub fn unsaved_titles(model: &Model) -> Vec<String> {
    model
        .documents
        .iter()
        .filter(|document| document.is_dirty())
        .map(|document| document.title.clone())
        .collect()
}

/// The answer to "these documents have unsaved changes" when a project is being left.
/// Save writes the ones that have a file and leaves the rest in the project as drafts,
/// which is where they were; Don't save closes them. Either way the switch goes on.
fn resolve_project_switch(model: &mut Model, choice: crate::model::CloseChoice) -> Vec<Effect> {
    use crate::model::CloseChoice;
    if !model.projects.asking_about_unsaved() {
        return Vec::new();
    }
    model.projects.dirty_choice = None;
    let mut effects = Vec::new();
    match choice {
        CloseChoice::Cancel => return update(model, Action::CancelProjectSwitch),
        CloseChoice::Save => {
            for document in model
                .documents
                .iter()
                .filter(|document| document.is_dirty())
            {
                if let Some(path) = &document.path {
                    effects.push(Effect::SaveDocument(crate::action::DocumentIoRequest {
                        document: document.id.clone(),
                        path: path.clone(),
                        content: document.text(),
                        revision: document.sql.revision(),
                        expected_fingerprint: None,
                    }));
                }
            }
        }
        CloseChoice::Discard => {
            let dirty: Vec<usize> = model
                .documents
                .iter()
                .enumerate()
                .filter(|(_, document)| document.is_dirty())
                .map(|(index, _)| index)
                .collect();
            for index in dirty.into_iter().rev() {
                let id = model.documents[index].id.clone();
                effects.extend(remove_document(model, index));
                effects.push(Effect::DiscardRecovery { document: id });
            }
        }
    }
    effects.extend(complete_switch_stage(model));
    effects
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
    if delete.typed.as_str() != delete.project.name {
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
    documents: Vec<dexo_storage::StoredDocument>,
    layout: Option<dexo_storage::WorkbenchLayout>,
    recent_sql_files: Vec<std::path::PathBuf>,
) {
    model.project = project.name.clone();
    model.project_id = project.id.0.to_string();
    model.projects.touch_recent(&project.name);
    model.projects.pending = None;
    // The switch is done; the dialog was only the way to it.
    model.projects.open = false;
    model.projects.intent = None;
    model.projects.error = None;
    model.messages.info(if model.projects.closing_sessions > 0 {
        format!(
            "Opened project {}. Its connections start closed; they connect when you use them.",
            project.name
        )
    } else {
        format!("Opened project {}.", project.name)
    });
    model.projects.closing_sessions = 0;
    model.recent_sql_files = recent_sql_files;
    if documents.is_empty() {
        model.documents = vec![crate::model::EditorDocument::placeholder()];
        model.active_document = 0;
    } else {
        // As at start-up: each comes back with its name, its connection and its kind.
        model.documents = documents.into_iter().map(document_from_stored).collect();
        model.active_document = 0;
    }
    apply_layout(model, layout);
    name_front_document_connection(model);
}

fn handle_projects_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::model::CloseChoice;
    use crate::screens::projects::ProjectsMode;
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    // The question about unsaved documents: the same three answers as closing a tab.
    if model.projects.asking_about_unsaved() {
        let choice = model.projects.dirty_choice.unwrap_or(CloseChoice::Save);
        return match key.code {
            KeyCode::Esc => resolve_project_switch(model, CloseChoice::Cancel),
            KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                model.projects.dirty_choice = Some(choice.next());
                Vec::new()
            }
            KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
                model.projects.dirty_choice = Some(choice.prev());
                Vec::new()
            }
            KeyCode::Enter => resolve_project_switch(model, choice),
            KeyCode::Char('s') => resolve_project_switch(model, CloseChoice::Save),
            KeyCode::Char('d') => resolve_project_switch(model, CloseChoice::Discard),
            _ => Vec::new(),
        };
    }
    // Deleting: type the name, Tab to the buttons. Alt+C: a plain `c` toggled this, so a
    // name with a `c` in it could never be typed to confirm.
    if let Some(delete) = &mut model.projects.delete {
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::ALT {
            delete.delete_connections = !delete.delete_connections;
            return Vec::new();
        }
        return match footer_key(&mut model.projects.footer, &key) {
            FooterKey::Cancel => {
                model.projects.delete = None;
                model.projects.mode = ProjectsMode::Browse;
                model.projects.footer = FooterFocus::Input;
                Vec::new()
            }
            FooterKey::Submit => update(model, Action::ConfirmProjectDelete),
            FooterKey::Moved => Vec::new(),
            FooterKey::Pass => {
                if model.projects.footer == FooterFocus::Input {
                    delete.typed.handle_key(key);
                }
                Vec::new()
            }
        };
    }
    match model.projects.mode {
        ProjectsMode::Create | ProjectsMode::Rename => {
            match footer_key(&mut model.projects.footer, &key) {
                FooterKey::Cancel => {
                    model.projects.mode = ProjectsMode::Browse;
                    model.projects.name_input.clear();
                    model.projects.error = None;
                    model.projects.footer = FooterFocus::Input;
                    // Asked for from the palette, there is no list to go back to.
                    if !model.projects.from_list {
                        model.projects.open = false;
                    }
                    return Vec::new();
                }
                FooterKey::Submit => return submit_project_name(model),
                FooterKey::Moved => return Vec::new(),
                FooterKey::Pass => {}
            }
            if model.projects.footer == FooterFocus::Input {
                model.projects.name_input.handle_key(key);
                model.projects.error = None;
            }
            Vec::new()
        }
        ProjectsMode::Browse | ProjectsMode::DeleteConfirm => match key.code {
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
                model.projects.selected = model.projects.selected.saturating_sub(1);
                model.projects.error = None;
                Vec::new()
            }
            KeyCode::Down => {
                if model.projects.selected + 1 < model.projects.list.len() {
                    model.projects.selected += 1;
                }
                model.projects.error = None;
                Vec::new()
            }
            KeyCode::Home => {
                model.projects.selected = 0;
                Vec::new()
            }
            KeyCode::End => {
                model.projects.selected = model.projects.list.len().saturating_sub(1);
                Vec::new()
            }
            KeyCode::Char('n') => {
                model.projects.mode = ProjectsMode::Create;
                model.projects.name_input.clear();
                model.projects.footer = FooterFocus::Input;
                model.projects.from_list = true;
                model.projects.error = None;
                Vec::new()
            }
            KeyCode::Char('r') => {
                model.projects.mode = ProjectsMode::Rename;
                let name = model
                    .projects
                    .selected()
                    .map(|project| project.name.clone())
                    .unwrap_or_default();
                model.projects.name_input.set_text(name);
                model.projects.footer = FooterFocus::Input;
                model.projects.from_list = true;
                model.projects.error = None;
                Vec::new()
            }
            KeyCode::Char('x') => update(model, Action::DeleteProject),
            _ => Vec::new(),
        },
    }
}

fn handle_config_transfer_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::screens::config_transfer::ConfigStage;
    use dexo_storage::ImportResolution;
    let stage = model.config_transfer.stage();
    let screen = &mut model.config_transfer;
    match key.code {
        KeyCode::Esc => match stage {
            ConfigStage::Start => {
                screen.open = false;
                Vec::new()
            }
            ConfigStage::ConfirmOverwrite | ConfigStage::Preview => {
                cancel_config_step(model);
                Vec::new()
            }
        },
        KeyCode::Char('e') if stage == ConfigStage::Start => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigExport,
            );
            Vec::new()
        }
        KeyCode::Char('i') if stage == ConfigStage::Start => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigImport,
            );
            Vec::new()
        }
        KeyCode::Left | KeyCode::BackTab => {
            screen.focus_prev();
            Vec::new()
        }
        KeyCode::Right | KeyCode::Tab => {
            screen.focus_next();
            Vec::new()
        }
        KeyCode::Up if stage == ConfigStage::Preview => {
            screen.select(-1);
            Vec::new()
        }
        KeyCode::Down if stage == ConfigStage::Preview => {
            screen.select(1);
            Vec::new()
        }
        KeyCode::PageUp if stage == ConfigStage::Preview => {
            screen.select(-5);
            Vec::new()
        }
        KeyCode::PageDown if stage == ConfigStage::Preview => {
            screen.select(5);
            Vec::new()
        }
        KeyCode::Char(' ') if stage == ConfigStage::Preview => {
            screen.cycle_selected();
            Vec::new()
        }
        KeyCode::Char('s') if stage == ConfigStage::Preview => {
            screen.resolve_selected(ImportResolution::Skip);
            Vec::new()
        }
        KeyCode::Char('o' | 'p') if stage == ConfigStage::Preview => {
            screen.resolve_selected(ImportResolution::Replace);
            Vec::new()
        }
        KeyCode::Char('r') if stage == ConfigStage::Preview => {
            screen.resolve_selected(ImportResolution::Rename(String::new()));
            Vec::new()
        }
        KeyCode::Enter => press_config_button(model),
        _ => Vec::new(),
    }
}

/// Backs out of the step the dialog is at: the question about replacing a file, or the
/// preview of an import. Nothing was changed by either.
fn cancel_config_step(model: &mut Model) {
    let screen = &mut model.config_transfer;
    screen.overwrite = None;
    screen.preview = None;
    screen.resolutions.clear();
    screen.focus = 0;
}

/// Enter on the focused button of the dialog, or a click on one.
fn press_config_button(model: &mut Model) -> Vec<Effect> {
    use crate::screens::config_transfer::ConfigStage;
    let screen = &model.config_transfer;
    let Some(label) = screen.buttons().get(screen.focus).copied() else {
        return Vec::new();
    };
    match (screen.stage(), label) {
        (ConfigStage::Start, "Export") => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigExport,
            );
            Vec::new()
        }
        (ConfigStage::Start, "Import") => {
            open_file_picker(
                model,
                crate::screens::file_picker::FilePickerMode::ConfigImport,
            );
            Vec::new()
        }
        (ConfigStage::Start, _) => {
            model.config_transfer.open = false;
            Vec::new()
        }
        (ConfigStage::ConfirmOverwrite, "Overwrite") => {
            let path = model.config_transfer.overwrite.clone().unwrap_or_default();
            update(model, Action::ExportConfig { path })
        }
        (ConfigStage::Preview, "Import") => update(model, Action::ApplyConfigImport),
        _ => {
            cancel_config_step(model);
            Vec::new()
        }
    }
}

fn open_project_intent(
    model: &mut Model,
    intent: crate::screens::projects::ProjectIntent,
) -> Vec<Effect> {
    open_projects(model, Some(intent))
}

/// The Projects dialog, fresh: whatever it was last left showing -- a half-typed name, an
/// error, a hint for another command -- is not carried into this opening.
fn open_projects(
    model: &mut Model,
    intent: Option<crate::screens::projects::ProjectIntent>,
) -> Vec<Effect> {
    model.projects.open = true;
    model.projects.intent = intent;
    model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
    model.projects.error = None;
    model.projects.delete = None;
    model.projects.name_input.clear();
    model.projects.footer = crate::widgets::form::FooterFocus::Input;
    model.projects.from_list = true;
    // The cursor starts on the project that is open.
    if let Some(index) = model
        .projects
        .list
        .iter()
        .position(|project| project.name == model.project)
    {
        model.projects.selected = index;
    }
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
        Some(crate::screens::projects::ProjectIntent::Rename) => {
            model.projects.mode = crate::screens::projects::ProjectsMode::Rename;
            model.projects.name_input.set_text(project.name);
            model.projects.footer = crate::widgets::form::FooterFocus::Input;
            model.projects.from_list = true;
            model.projects.error = None;
            Vec::new()
        }
        Some(crate::screens::projects::ProjectIntent::Delete) => {
            update(model, Action::DeleteProject)
        }
        Some(crate::screens::projects::ProjectIntent::Switch) | None => {
            if project.name == model.project {
                model.projects.open = false;
                model.projects.intent = None;
                model
                    .messages
                    .info(format!("Project {} is already open.", project.name));
                return Vec::new();
            }
            update(model, Action::SwitchProject { name: project.name })
        }
    }
}

/// The connections screen, with the databases running in Docker looked for again.
fn open_connections(model: &mut Model) -> Vec<Effect> {
    model.connections.open = true;
    vec![Effect::DiscoverDocker]
}

fn choose_connection_intent(model: &mut Model) -> Vec<Effect> {
    // A database running in Docker opens the New Connection form filled in from it;
    // nothing is saved, nor its password kept, until the form is saved.
    if let Some(database) = model.connections.selected_docker().cloned() {
        let mut form = crate::screens::connection::ConnectionForm::open();
        let connection = &database.connection;
        form.set_value("driver", &connection.driver);
        form.sync_descriptor_fields();
        let port = connection
            .port
            .map(|port| port.to_string())
            .unwrap_or_default();
        for (label, value) in [
            ("name", connection.name.as_str()),
            ("host", connection.host.as_str()),
            ("port", port.as_str()),
            ("database", connection.database.as_str()),
            ("username", connection.username.as_str()),
        ] {
            form.set_value(label, value);
        }
        if let Some(password) = &database.password {
            form.set_value("password", secrecy::ExposeSecret::expose_secret(password));
        }
        form.allow_empty_password = database.passwordless;
        model.connection_form = form;
        return Vec::new();
    }
    if model.connections.selected().is_none() {
        model.connections.error = Some("select a connection first".into());
        return Vec::new();
    }
    connect_selected(model)
}

fn open_snippets(model: &mut Model) -> Vec<Effect> {
    // Always read: what comes back is the person's snippets with the built-in ones.
    model.editor.snippet_pending = true;
    vec![Effect::LoadSnippets]
}

fn open_parameters(model: &mut Model) -> Vec<Effect> {
    // Reading the statement again forgets the values; each parameter keeps its own, so
    // Edit Parameters starts from what was given.
    let given = std::mem::take(&mut model.editor.parameters);
    crate::screens::editor::refresh_intelligence(model, false);
    for parameter in &mut model.editor.parameters {
        if let Some(before) = given.iter().find(|before| before.name == parameter.name) {
            parameter.value = before.value.clone();
        }
    }
    if model.editor.parameters.is_empty() {
        model.messages.warn("no query parameters".into());
        return Vec::new();
    }
    crate::screens::editor::begin_parameter_prompt(model);
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

/// Asks first, whether or not the list has been read: the palette used to refuse with
/// "history is empty" until Search History had loaded it.
fn open_clear_history(model: &mut Model) -> Vec<Effect> {
    model.editor.history_open = true;
    model.editor.history_confirm_clear = true;
    // Cancel holds the focus: an Enter out of habit keeps the history.
    model.editor.history_footer = crate::widgets::form::FooterFocus::Cancel;
    Vec::new()
}

fn confirm_clear_history(model: &mut Model) -> Vec<Effect> {
    let connection_id = model.connection.name.clone();
    model.editor.history_confirm_clear = false;
    model.editor.history_open = false;
    model.editor.history.clear();
    model.editor.history_selected = 0;
    model.messages.info(if connection_id.is_empty() {
        "History cleared.".into()
    } else {
        format!("History of {connection_id} cleared.")
    });
    vec![Effect::ClearHistory { connection_id }]
}

/// Enter in the history: the statement opens in a new document of the connection, as a
/// saved query does, and does not run. It used to replace the active document, unsaved
/// work included, and run at once.
fn open_history_entry(model: &mut Model) -> Vec<Effect> {
    let Some(sql) = crate::screens::editor::picked_history(model) else {
        model.editor.history_open = false;
        return Vec::new();
    };
    model.editor.history_open = false;
    let name = suggested_document_name(model);
    open_text_document(model, &name, &sql, None)
}

/// A new document holding `text`, on the connection `connection` names, or else the one
/// the active document is on. Nothing runs, and nothing already open is replaced.
pub(crate) fn open_text_document(
    model: &mut Model,
    title: &str,
    text: &str,
    connection: Option<&str>,
) -> Vec<Effect> {
    let named = connection.and_then(|name| {
        model
            .connections
            .profiles
            .iter()
            .find(|row| row.profile.name == name)
            .map(|row| row.profile.id.0.to_string())
    });
    let connection = named
        .or_else(|| model.active_document().connection_id.clone())
        .or_else(|| active_connection_uuid(model));
    let title = if model
        .documents
        .iter()
        .any(|document| document.title.eq_ignore_ascii_case(title))
    {
        suggested_document_name(model)
    } else {
        title.to_string()
    };
    let mut document = crate::model::EditorDocument::new_unique(title, None, connection);
    document.sql = dexo_sql::SqlDocument::new(text);
    model.documents.push(document);
    let index = model.documents.len() - 1;
    let effects = activate_document(model, index);
    model.focus_active_document_tab();
    model.focus = Focus::Editor;
    effects
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
    model
        .file_picker
        .fit_recents(crate::screens::file_picker::inner_rows(model.height));
    // A config file is looked for in the home folder, not wherever Dexo was started from.
    if matches!(
        mode,
        crate::screens::file_picker::FilePickerMode::ConfigExport
            | crate::screens::file_picker::FilePickerMode::ConfigImport
    ) && let Some(home) = std::env::home_dir()
    {
        let _ = model.file_picker.enter_path(home);
    }
    if mode == crate::screens::file_picker::FilePickerMode::ConfigExport {
        model.file_picker.name.set_text("dexo-config.toml");
    }
    if mode == crate::screens::file_picker::FilePickerMode::Save {
        // The picker only opens for a document that has never been saved, so the file
        // name alone left the field empty every time; the tab's name is the one to offer.
        let document = model.active_document();
        let preset = document
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| document.title.clone());
        if !preset.trim().is_empty() {
            model.file_picker.name.set_text(preset);
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
    // The recent list takes the file once it has been read: a file that could not be
    // opened is not one to offer again.
    vec![Effect::LoadDocument(crate::action::DocumentIoRequest {
        document: document_id,
        path: normalized,
        content: String::new(),
        revision: 0,
        expected_fingerprint: None,
    })]
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
            model.projects.delete = None;
            model.projects.from_list = false;
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
        PaletteInvocation::OpenFlow(FlowIntent::DataReview) => update(model, Action::OpenReview),
        // The fields come first: previewing straight away previewed a form nobody saw.
        PaletteInvocation::OpenFlow(FlowIntent::SchemaPreview) => {
            if model.connection.read_only {
                model.messages.warn(format!(
                    "{} is read-only: a schema change cannot be applied here.",
                    model.connection.name
                ));
                return Vec::new();
            }
            model.schema_editor.raw_sql.clear();
            model.schema_editor.form_diff = None;
            model.schema_editor.errors.clear();
            model.schema_editor.footer = crate::widgets::form::FooterFocus::Input;
            model.schema_editor.open = true;
            Vec::new()
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
    let visible = crate::palette::filter_entries(&entries, model.palette.query.as_str());
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

    /// A completion list open when a statement ran stayed on screen, over the
    /// production confirmation the run brought up.
    #[test]
    fn running_a_statement_closes_the_completion_list() {
        let mut model = Model {
            focus: Focus::Editor,
            ..Model::default()
        };
        model
            .active_document_mut()
            .sql
            .insert(0, "select 1")
            .unwrap();
        model.editor.completion_open = true;
        update(&mut model, Action::ExecuteStatement);
        assert!(!model.editor.completion_open);
        model.editor.completion_open = true;
        update(&mut model, Action::OpenExplain);
        assert!(!model.editor.completion_open);
    }

    /// A closed or renamed document left a number that counting the documents gave
    /// out again, to a name still open.
    #[test]
    fn a_new_document_is_offered_a_name_no_open_one_has() {
        let mut model = crate::model::Model::default();
        model.documents = ["query-2.sql", "Query-1.sql", "notes.sql"]
            .into_iter()
            .map(|title| crate::model::EditorDocument::new_unique(title, None, None))
            .collect();
        assert_eq!(super::suggested_document_name(&model), "query-3.sql");
        model.documents.remove(0);
        assert_eq!(super::suggested_document_name(&model), "query-2.sql");
        super::open_new_document_prompt(&mut model);
        assert!(model.document_name_prompt.name.is_selected());
        let screen = crate::render::render_to_string(&model, 80, 24);
        assert!(screen.contains("query-2.sql"), "{screen}");
    }
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

        assert_eq!(model.help.query.as_str(), "d");
        assert_eq!(model.help.scroll, 0);
    }

    #[test]
    fn backspace_while_help_is_open_removes_last_search_char() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("disc");

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        );

        assert_eq!(model.help.query.as_str(), "dis");
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
        assert_eq!(model.help.query.as_str(), "?");
    }

    #[test]
    fn esc_still_closes_help_and_clears_its_search_query() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("disc");

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );

        assert!(!model.help.open);
        assert_eq!(model.help.query.as_str(), "");
    }

    #[test]
    fn reopening_help_resets_a_previous_search_query() {
        let mut model = Model::default();
        model.help.open = true;
        model.help.query = crate::widgets::text_input::TextInput::new("disc");

        update(&mut model, Action::ToggleHelp);
        assert!(!model.help.open);
        update(&mut model, Action::ToggleHelp);

        assert!(model.help.open);
        assert_eq!(model.help.query.as_str(), "");
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
    fn save_connection_keeps_password_out_of_debug_and_emits_create() {
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

    /// `dexo <url>`: the profile exists only in the model, cannot be duplicated, moved
    /// or deleted as if it were saved, and once saved its documents follow it.
    #[test]
    fn a_temporary_connection_is_listed_dialled_and_saved_with_its_documents() {
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

        let temporary = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(7)),
            None,
            "ana@db/shop",
            "postgres",
            "local",
            serde_json::json!({"host": "db"}),
            SecretRef::new("memory-only".into()),
        );
        let mut model = Model::default();
        let effects = update(
            &mut model,
            Action::OpenTemporaryConnection(Box::new(temporary.clone())),
        );
        assert!(matches!(&effects[..], [Effect::ConnectProfile { .. }]));
        assert!(model.connections.profiles[0].temporary);
        for action in [
            Action::DuplicateConnection,
            Action::DeleteConnection,
            Action::MoveConnectionGroup { group: "x".into() },
        ] {
            assert!(update(&mut model, action).is_empty());
        }
        assert!(model.connections.delete_target.is_none());
        assert!(matches!(
            &update(&mut model, Action::EditSelectedConnection)[..],
            [Effect::RevealTemporarySecret { .. }]
        ));

        model.active_document_mut().connection_id = Some(temporary.id.0.to_string());
        let mut saved = temporary.clone();
        saved.id = ConnectionId(uuid::Uuid::from_u128(8));
        model.connection_form.saving_temporary = Some(temporary.clone());
        update(&mut model, Action::ProfileSaved(saved.clone()));
        assert!(model.connections.temporary.is_empty());
        assert!(!model.connections.profiles[0].temporary);
        assert!(!model.connection_form.open);
        assert_eq!(
            model.active_document().connection_id.as_deref(),
            Some(saved.id.0.to_string().as_str())
        );
    }

    /// Saved under another name, a temporary connection's session goes by the new one;
    /// and no other connection may take a temporary one's name while it is open.
    #[test]
    fn a_temporary_connection_saved_under_another_name_keeps_its_session() {
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

        let temporary = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(7)),
            None,
            "demo",
            "sqlite",
            "local",
            serde_json::json!({"path": "/tmp/demo.db"}),
            SecretRef::new("memory-only".into()),
        );
        let mut model = Model::default();
        update(
            &mut model,
            Action::OpenTemporaryConnection(Box::new(temporary.clone())),
        );
        let session = crate::runtime::SessionId(uuid::Uuid::from_u128(1));
        model
            .connections
            .upsert_session(crate::screens::connections::SessionRow {
                id: session,
                connection: "demo".into(),
                transaction: dexo_driver_api::TransactionState::Idle,
                generation: 1,
                environment: "local".into(),
                read_only: false,
                driver: "sqlite".into(),
            });
        model.connection.name = "demo".into();

        // A new connection may not be called "demo" while the demo is open.
        model.connection_form = crate::screens::connection::ConnectionForm::open();
        for (label, value) in [
            ("name", "demo"),
            ("host", "db"),
            ("database", "shop"),
            ("username", "ana"),
            ("password", "secret"),
        ] {
            model.connection_form.set_value(label, value);
        }
        assert!(update(&mut model, Action::SaveConnection).is_empty());
        assert!(!model.connection_form.errors.is_empty());
        // Nor may a saved one be renamed to it.
        model.connection_form = crate::screens::connection::ConnectionForm::open();
        model.connection_form.editing = Some(ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(9)),
            None,
            "pg",
            "postgres",
            "local",
            serde_json::json!({"host": "db"}),
            SecretRef::new("ref-9".into()),
        ));
        for (label, value) in [
            ("name", "demo"),
            ("host", "db"),
            ("database", "shop"),
            ("username", "ana"),
        ] {
            model.connection_form.set_value(label, value);
        }
        assert!(update(&mut model, Action::SaveConnection).is_empty());
        assert!(!model.connection_form.errors.is_empty());

        model.connection_form.saving_temporary = Some(temporary.clone());
        let mut saved = temporary.clone();
        saved.id = ConnectionId(uuid::Uuid::from_u128(8));
        saved.name = "shop".into();
        let effects = update(&mut model, Action::ProfileSaved(saved));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::RenameSessions { from, to } if from == "demo" && to == "shop"
        )));
        assert!(model.connections.temporary.is_empty());
        assert_eq!(model.connection.name, "shop");
        assert_eq!(model.connections.sessions[0].connection, "shop");
    }

    /// The warning a temporary connection was opened with is said when that one
    /// connects, not when another does first.
    #[test]
    fn a_startup_warning_waits_for_its_own_connection() {
        let mut model = Model {
            startup_warning: Some(("demo".into(), "the URL's password shows".into())),
            ..Model::default()
        };
        let connected = |model: &mut Model, name: &str, token: u64| {
            model.connections.pending_connect = Some(token);
            update(
                model,
                Action::ConnectionChanged {
                    name: name.into(),
                    ready: true,
                    environment: "local".into(),
                    session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(
                        token.into(),
                    ))),
                    generation: token,
                    token,
                    read_only: false,
                    driver: "sqlite".into(),
                },
            );
            model
                .messages
                .last()
                .map(|last| last.message.clone())
                .unwrap_or_default()
        };
        assert_eq!(connected(&mut model, "prod", 1), "Connected to prod");
        assert_eq!(connected(&mut model, "demo", 2), "the URL's password shows");
        assert!(model.startup_warning.is_none());
    }

    /// The inspector shows the note on an object -- the person's, else the database's
    /// comment -- and `n` writes one for the connection it belongs to.
    #[test]
    fn the_inspector_shows_and_writes_notes() {
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model::default();
        model.connections.load_profiles(vec![ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(4)),
            None,
            "shop",
            "postgres",
            "local",
            serde_json::json!({"host": "h"}),
            SecretRef::new("r".into()),
        )]);
        model.connection.name = "shop".into();
        model.inspector.open = true;
        model.inspector.qualified_name = "shop.public.orders".into();
        model.inspector.object = Some(
            dexo_driver_api::CatalogObject::new(
                dexo_driver_api::ObjectId::new("orders"),
                dexo_driver_api::ObjectKind::Table,
                dexo_driver_api::QualifiedName::new(Some("shop"), Some("public"), "orders"),
                None,
            )
            .with_attribute("comment", serde_json::json!("orders placed online")),
        );
        let screen = crate::render::render_to_string(&model, 120, 30);
        assert!(
            screen.contains("orders placed online (database comment)"),
            "{screen}"
        );
        update(
            &mut model,
            Action::NoteLoaded {
                object: "shop.public.orders".into(),
                note: Some("One row per checkout.".into()),
            },
        );
        assert!(
            crate::render::render_to_string(&model, 120, 30)
                .contains("note: One row per checkout.")
        );
        update(&mut model, key(KeyCode::Char('n')));
        for ch in " Paid only.".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        let effects = update(&mut model, key(KeyCode::Enter));
        let connection = uuid::Uuid::from_u128(4).to_string();
        assert!(
            matches!(
                effects.as_slice(),
                [Effect::SaveNote { connection_id, object, note }]
                    if *connection_id == connection
                        && object == "shop.public.orders"
                        && note == "One row per checkout. Paid only."
            ),
            "{effects:?}"
        );
        // Shown once the save answers, not before; a failed save says so and shows
        // nothing new.
        assert_eq!(
            model.inspector.note.as_deref(),
            Some("One row per checkout.")
        );
        update(
            &mut model,
            Action::NoteSaved {
                object: "shop.public.orders".into(),
                saved: Err("disk full".into()),
            },
        );
        assert_eq!(
            model.inspector.note.as_deref(),
            Some("One row per checkout.")
        );
        assert!(
            model
                .messages
                .last()
                .is_some_and(|message| message.message.contains("was not saved: disk full"))
        );
        update(
            &mut model,
            Action::NoteSaved {
                object: "shop.public.orders".into(),
                saved: Ok(Some("One row per checkout. Paid only.".into())),
            },
        );
        assert_eq!(
            model.inspector.note.as_deref(),
            Some("One row per checkout. Paid only.")
        );

        // [Save] and [Cancel] answer the mouse; a click beside them keeps the draft.
        update(&mut model, key(KeyCode::Char('n')));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        let mut hits = crate::mouse::HitMap::default();
        terminal
            .draw(|frame| crate::render::render(frame, &model, &mut hits))
            .unwrap();
        let click = |(column, row): (u16, u16)| {
            Action::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let save = hits.center(crate::mouse::HitTarget::FooterSubmit);
        let cancel = hits.center(crate::mouse::HitTarget::FooterCancel);
        model.hits = hits;
        update(&mut model, click((save.0 + 20, save.1)));
        assert!(model.inspector.open && model.inspector.editing_note.is_some());
        assert!(matches!(
            update(&mut model, click(save)).as_slice(),
            [Effect::SaveNote { .. }]
        ));
        update(&mut model, key(KeyCode::Char('n')));
        update(&mut model, click(cancel));
        assert!(model.inspector.editing_note.is_none() && model.inspector.open);
    }

    /// The palette's Edit Object Note opens the inspector on the explorer's object and
    /// the note editor once the object and its note are read.
    #[test]
    fn the_palette_edits_an_objects_note() {
        let session = crate::runtime::SessionId(uuid::Uuid::from_u128(1));
        let mut model = Model {
            active_session: Some(session),
            ..Model::default()
        };
        model.explorer.replace_roots(dexo_driver_api::CatalogList {
            objects: vec![dexo_driver_api::CatalogObject::new(
                dexo_driver_api::ObjectId::new("orders"),
                dexo_driver_api::ObjectKind::Table,
                dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders"),
                None,
            )],
            restrictions: vec![],
        });
        model
            .explorer
            .select(dexo_driver_api::ObjectId::new("orders"));
        let effects = update(&mut model, Action::EditObjectNote);
        assert!(
            matches!(effects.as_slice(), [Effect::LoadObjectInspector { .. }]),
            "{effects:?}"
        );
        assert!(model.inspector.open && model.inspector.editing_note.is_none());
        let generation = model.session_generation;
        update(
            &mut model,
            Action::InspectorLoaded {
                generation,
                session: session.0.to_string(),
                qualified_name: "public.orders".into(),
                object: Some(dexo_driver_api::CatalogObject::new(
                    dexo_driver_api::ObjectId::new("orders"),
                    dexo_driver_api::ObjectKind::Table,
                    dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders"),
                    None,
                )),
                ddl: None,
                dependencies: Vec::new(),
                dependents: Vec::new(),
                names: Default::default(),
                effective_privileges: Vec::new(),
                restrictions: Vec::new(),
            },
        );
        assert!(model.inspector.editing_note.is_some());
        let entry = crate::palette::palette_entries(&model)
            .into_iter()
            .find(|entry| entry.id == "explorer.note")
            .expect("in the palette");
        assert_eq!(entry.shortcut.as_deref(), Some("Shift+N"));
    }

    /// Under a DDL taller than the inspector, `n` opens the note editor in view, not
    /// below the popup where the note was typed blind.
    #[test]
    fn the_note_editor_opens_in_view() {
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model::default();
        model.inspector.open = true;
        model.inspector.facet = crate::screens::object_inspector::InspectorFacet::Ddl;
        model.inspector.qualified_name = "shop.public.orders".into();
        model.inspector.ddl = Some(
            (0..60)
                .map(|column| format!("  column_{column} integer,"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        model.inspector.object = Some(dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new("orders"),
            dexo_driver_api::ObjectKind::Table,
            dexo_driver_api::QualifiedName::new(Some("shop"), Some("public"), "orders"),
            None,
        ));
        update(&mut model, key(KeyCode::Char('n')));
        for ch in "typed here".chars() {
            update(&mut model, key(KeyCode::Char(ch)));
        }
        let screen = crate::render::render_to_string(&model, 120, 30);
        assert!(screen.contains("note: typed here"), "{screen}");
        assert!(screen.contains("[Save]"), "{screen}");
    }

    /// Agent Activity lists a waiting write with its SQL; approving takes a deliberate
    /// second step (Cancel holds the focus), denying one Enter; a request waiting while
    /// the screen is closed is announced once.
    #[test]
    fn agent_activity_decides_waiting_writes() {
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let request = dexo_app::mcp::Approval::pending(
            "assistant",
            "local",
            "data_execute_sql",
            serde_json::json!({"sql": "DELETE FROM orders WHERE id = 7"})
                .as_object()
                .unwrap(),
            vec!["db.public.orders".into()],
            1000,
            120,
        );
        let mut model = Model::default();
        update(&mut model, Action::ApprovalsWaiting(vec![request.clone()]));
        assert!(
            model
                .messages
                .last()
                .is_some_and(|message| message.message.contains("waiting for your approval"))
        );
        let shown = model.messages.len();
        update(&mut model, Action::ApprovalsWaiting(vec![request.clone()]));
        assert_eq!(model.messages.len(), shown, "said twice");

        assert!(matches!(
            update(&mut model, Action::OpenMcpAudit).as_slice(),
            [Effect::LoadMcpAudit]
        ));
        update(
            &mut model,
            Action::McpAuditLoaded {
                events: vec![
                    "assistant grant data_execute_sql ask db.public.orders waiting".into(),
                ],
                pending: vec![request.clone()],
                now: 1010,
            },
        );
        let lines = model.mcp_audit.lines().join("\n");
        assert!(lines.contains("DELETE FROM orders WHERE id = 7"), "{lines}");
        assert!(lines.contains("2 min left"), "{lines}");
        update(&mut model, key(KeyCode::Char('a')));
        assert!(
            update(&mut model, key(KeyCode::Enter)).is_empty(),
            "Enter alone approved"
        );
        update(&mut model, key(KeyCode::Char('a')));
        update(&mut model, key(KeyCode::Left));
        assert!(matches!(
            update(&mut model, key(KeyCode::Enter)).as_slice(),
            [Effect::SettleApproval { id, approve: true }] if *id == request.id
        ));
        update(&mut model, key(KeyCode::Char('d')));
        assert!(matches!(
            update(&mut model, key(KeyCode::Enter)).as_slice(),
            [Effect::SettleApproval { approve: false, .. }]
        ));
        assert!(matches!(
            update(&mut model, Action::AgentActivityTick).as_slice(),
            [Effect::LoadMcpAudit]
        ));
    }

    /// MCP Profiles makes a grant the way `dexo mcp grant create` does, "ask before each
    /// write" included: the form asks for the request the CLI's `--ask` makes.
    #[test]
    fn a_grant_that_asks_is_made_from_mcp_profiles() {
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let mut model = Model::default();
        model.connection.name = "local".into();
        update(&mut model, Action::OpenMcpProfiles);
        update(
            &mut model,
            Action::McpProfilesLoaded {
                profiles: vec![crate::screens::mcp_profiles::McpProfileSummary {
                    name: "assistant".into(),
                    enabled: true,
                    ..Default::default()
                }],
            },
        );
        update(&mut model, key(KeyCode::Char('g')));
        assert!(model.mcp_profiles.grant_form.is_some());
        let typed = |model: &mut Model, text: &str| {
            for ch in text.chars() {
                update(model, key(KeyCode::Char(ch)));
            }
            update(model, key(KeyCode::Down));
        };
        // profile, then connection (free text: the profile lists none), then capability.
        update(&mut model, key(KeyCode::Down));
        typed(&mut model, "local");
        update(&mut model, key(KeyCode::Down));
        typed(&mut model, "data_insert");
        typed(&mut model, "db.public.items");
        update(&mut model, key(KeyCode::Down));
        // "ask before each write", then its timeout, then the confirmation.
        update(&mut model, key(KeyCode::Char(' ')));
        update(&mut model, key(KeyCode::Down));
        for _ in 0..3 {
            update(&mut model, key(KeyCode::Backspace));
        }
        typed(&mut model, "90");
        typed(&mut model, "local");
        let lines = model
            .mcp_profiles
            .grant_form
            .as_ref()
            .unwrap()
            .lines()
            .join("\n");
        assert!(lines.contains("[x] each write waits for you"), "{lines}");
        let effects = update(&mut model, key(KeyCode::Enter));
        let expected = dexo_app::mcp::GrantRequest {
            connection: "local".into(),
            capability: "data_write".into(),
            tools: vec!["data_insert".into()],
            selector: "db.public.items".into(),
            expires: "15m".into(),
            confirm_target: "local".into(),
            ask_secs: Some(90),
        };
        assert!(
            matches!(
                effects.as_slice(),
                [Effect::CreateMcpGrant { profile, request }]
                    if profile == "assistant" && *request == expected
            ),
            "{effects:?}"
        );
        update(
            &mut model,
            Action::McpGrantFailed {
                message: "connection is not allowed for this profile".into(),
            },
        );
        let form = model.mcp_profiles.grant_form.as_ref().expect("stays open");
        assert!(form.lines().join("\n").contains("not allowed"));
        update(&mut model, key(KeyCode::Esc));
        assert!(model.mcp_profiles.grant_form.is_none() && model.mcp_profiles.open);
        let entry = crate::palette::palette_entries(&model)
            .into_iter()
            .find(|entry| entry.id == "mcp.grant")
            .expect("in the palette");
        assert_eq!(
            entry.shortcut, None,
            "`g` is not a key outside MCP Profiles"
        );
    }

    /// The statement being approved is shown whole: its seventh line and the tail of a
    /// long one are reached by scrolling, wrapped, while [Approve]/[Cancel] stay on
    /// screen however long the list and short the terminal.
    #[test]
    fn agent_activity_shows_the_whole_statement_and_keeps_its_buttons() {
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let request = |sql: &str| {
            dexo_app::mcp::Approval::pending(
                "assistant",
                "local",
                "data_execute_sql",
                serde_json::json!({ "sql": sql }).as_object().unwrap(),
                vec!["db.public.orders".into()],
                1000,
                120,
            )
        };
        let long = format!(
            "UPDATE orders SET note = '{}' WHERE id = 2 OR 1 = 1",
            "x".repeat(150)
        );
        let sql = format!(
            "UPDATE orders SET paid = true\nWHERE id = 1\nAND a\nAND b\nAND c\nAND d\nOR TRUE;\n{long}"
        );
        let mut pending = vec![request(&sql)];
        pending.extend((0..8).map(|table| request(&format!("DELETE FROM t{table}"))));
        let mut model = Model::default();
        update(
            &mut model,
            Action::Resize {
                width: 80,
                height: 16,
            },
        );
        model.mcp_audit.open = true;
        update(
            &mut model,
            Action::McpAuditLoaded {
                events: Vec::new(),
                pending,
                now: 1010,
            },
        );
        update(&mut model, key(KeyCode::Char('a')));
        let mut seen = String::new();
        for _ in 0..12 {
            let frame = crate::render::render_to_string(&model, 80, 16);
            assert!(
                frame.contains("[Approve]") && frame.contains("[Cancel]"),
                "{frame}"
            );
            seen.push_str(&frame);
            update(&mut model, key(KeyCode::PageDown));
        }
        assert!(seen.contains("OR TRUE"), "{seen}");
        assert!(seen.contains("WHERE id = 2 OR 1 = 1"), "{seen}");
    }

    /// A database running in Docker is listed under the saved connections, and Enter on
    /// it opens the New Connection form filled in from it -- saving nothing.
    #[test]
    fn a_docker_database_prefills_the_connection_form() {
        let mut model = Model::default();
        let effects = update(&mut model, Action::OpenConnections);
        assert!(matches!(effects.as_slice(), [Effect::DiscoverDocker]));
        let found = dexo_app::docker::from_inspect(
            r#"[{"Name": "/shop-pg", "Config": {"Image": "postgres:16",
                 "Env": ["POSTGRES_USER=ana", "POSTGRES_PASSWORD=s3cret", "POSTGRES_DB=shop"]},
               "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "5433"}]}}}]"#,
        );
        update(&mut model, Action::DockerDiscovered(found));
        let screen = crate::render::render_to_string(&model, 100, 30);
        assert!(screen.contains("Running in Docker"), "{screen}");
        assert!(
            screen.contains("shop-pg [postgres] 127.0.0.1:5433"),
            "{screen}"
        );
        assert!(
            update(
                &mut model,
                Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            )
            .is_empty()
        );
        let form = &model.connection_form;
        assert!(form.open);
        let value = |label: &str| {
            form.fields
                .iter()
                .find(|field| field.label == label)
                .map(|field| field.value.as_str().to_string())
                .unwrap_or_default()
        };
        assert_eq!(
            [
                "name", "driver", "host", "port", "database", "username", "password"
            ]
            .map(value),
            [
                "shop-pg",
                "postgres",
                "127.0.0.1",
                "5433",
                "shop",
                "ana",
                "s3cret"
            ]
            .map(String::from)
        );
        // A save the app turns down comes back to the form with the password as it was.
        let effects = update(&mut model, Action::SaveConnection);
        assert!(
            matches!(effects.as_slice(), [Effect::CreateConnection { password, .. }] if password == "s3cret")
        );
        update(
            &mut model,
            Action::ConnectionFormError {
                message: "connection 'shop-pg' already exists".into(),
            },
        );
        assert!(
            model
                .connection_form
                .fields
                .iter()
                .any(|field| field.label == "password" && field.value.as_str() == "s3cret")
        );
    }

    /// A sort's re-run that fails puts the rows back in its own document, even when
    /// another document is on screen by then.
    #[test]
    fn a_failed_re_run_comes_back_to_its_own_document() {
        let mut model = Model {
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            session_generation: 1,
            ..Model::default()
        };
        model.documents = vec![
            crate::model::EditorDocument::with_text("select id from a"),
            crate::model::EditorDocument::with_text("select 2"),
        ];
        model.documents[0].id = "doc-a".into();
        model.documents[1].id = "doc-b".into();
        model.active_document = 0;
        update(&mut model, Action::SelectDocument { index: 0 });
        let mut tab = crate::model::ResultTab::new(
            crate::model::ResultKey {
                operation: crate::runtime::OperationKey::new(
                    crate::runtime::OperationId::new(),
                    model.active_session.unwrap().0.to_string(),
                    "doc-a",
                    1,
                ),
                index: 0,
            },
            "a's rows",
        );
        tab.source_sql = Some("select id from a".into());
        model.results.tabs = vec![tab];
        model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
            name: "id".into(),
            type_name: "int".into(),
            nullable: false,
        }]);
        model.results.append_rows(vec![vec![DbValue::I64(1)]]);
        let effects = update(
            &mut model,
            Action::SortByColumn {
                column: Some(0),
                add: false,
            },
        );
        let key = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::StartScript(request) => Some(request.key.clone()),
                _ => None,
            })
            .expect("the sort runs again");
        model.active_operation = None;
        update(&mut model, Action::SelectDocument { index: 1 });
        update(
            &mut model,
            Action::QueryFailed {
                key,
                index: 0,
                message: "no".into(),
                details: Vec::new(),
                position: None,
            },
        );
        assert!(model.results.tabs.iter().all(|tab| tab.title != "a's rows"));
        update(&mut model, Action::SelectDocument { index: 0 });
        assert_eq!(model.results.tabs[0].title, "a's rows");
        assert_eq!(model.results.rows().len(), 1);
        assert!(model.data.bars.applied.order_by.is_none());
    }

    /// A page answers only the request the grid last made: an older page, or its late
    /// failure, neither fills the grid nor resets the WHERE that runs now.
    #[test]
    fn only_the_last_page_asked_for_lands() {
        let session = crate::runtime::SessionId(uuid::Uuid::from_u128(1));
        let mut model = Model {
            active_session: Some(session),
            session_generation: 1,
            ..Model::default()
        };
        model.documents = vec![crate::model::EditorDocument::new_table(
            dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders"),
            None,
        )];
        model.active_document = 0;
        super::load_table_document(&mut model, 0);
        let first = model.data.page_ticket.expect("a page is asked for");
        model.data.bars.where_input.set_text("id > 1");
        assert!(!super::apply_clauses(&mut model).is_empty());
        let second = model.data.page_ticket.unwrap();
        assert_ne!(first, second);
        let page = |rows: i64| {
            dexo_driver_api::DataPage::from_fetched(
                vec![dexo_driver_api::ColumnMeta {
                    name: "id".into(),
                    type_name: "int".into(),
                    nullable: false,
                }],
                (0..rows).map(|row| vec![DbValue::I64(row)]).collect(),
                0,
                100,
            )
        };
        update(
            &mut model,
            Action::DataPageFailed {
                generation: 1,
                ticket: first,
                message: "late".into(),
            },
        );
        assert_eq!(model.data.bars.applied.where_sql.as_deref(), Some("id > 1"));
        update(
            &mut model,
            Action::DataPageLoaded {
                generation: 1,
                session: session.0.to_string(),
                ticket: first,
                page: page(5),
            },
        );
        assert_eq!(model.results.row_count(), 0);
        update(
            &mut model,
            Action::DataPageLoaded {
                generation: 1,
                session: session.0.to_string(),
                ticket: second,
                page: page(2),
            },
        );
        assert_eq!(model.results.row_count(), 2);
    }

    /// SQLite keeps a REFERENCES as written: `Customers` is the `customers` table.
    /// Postgres's names are exact.
    #[test]
    fn a_key_written_in_another_case_still_links_on_sqlite() {
        let table = dexo_driver_api::QualifiedName::new(None::<String>, Some("main"), "customers");
        let key = dexo_driver_api::ForeignKeyRef {
            name: "orders_customer".into(),
            from: dexo_driver_api::QualifiedName::new(None::<String>, Some("main"), "orders"),
            from_columns: vec!["customer_id".into()],
            to: dexo_driver_api::QualifiedName::new(None::<String>, Some("main"), "Customers"),
            to_columns: vec!["id".into()],
        };
        let keys = [key];
        assert_eq!(
            super::related_links(&table, &keys, dexo_sql::Dialect::Sqlite).len(),
            1
        );
        assert!(super::related_links(&table, &keys, dexo_sql::Dialect::Postgres).is_empty());
    }

    /// Enter while the keys are still being read waits; an answer from a replaced
    /// session closes the picker and says so instead of waiting for ever.
    #[test]
    fn the_related_picker_never_waits_for_ever() {
        let table = dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders");
        let mut model = Model {
            session_generation: 2,
            ..Model::default()
        };
        model.data.related_picker = Some(crate::screens::data::RelatedPicker {
            table: table.clone(),
            links: None,
            selected: 0,
        });
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(model.data.related_picker.is_some());
        update(
            &mut model,
            Action::ForeignKeysLoaded {
                generation: 1,
                table,
                result: Ok(Vec::new()),
            },
        );
        assert!(model.data.related_picker.is_none());
        assert!(
            model
                .messages
                .iter()
                .any(|entry| entry.message.contains("press f again"))
        );
    }

    /// In the Vim keymap, Save Query As takes the Visual selection on screen, not the
    /// editor's own leftover range.
    #[test]
    fn save_query_takes_vims_visual_selection() {
        let mut model = Model {
            keymap: crate::keymap::Keymap::vim_profile(),
            focus: Focus::Editor,
            project_id: "p".into(),
            ..Model::default()
        };
        model.set_sql("select 1;\nselect 2;");
        model.active_document_mut().sql.set_cursor(0).unwrap();
        model.connection.name = "pg".into();
        model
            .connections
            .load_profiles(vec![dexo_app::ConnectionProfile::new(
                dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
                None,
                "pg",
                "postgres",
                "local",
                serde_json::json!({"host": "h"}),
                dexo_app::connection_profile::SecretRef::new("ref".into()),
            )]);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::NONE)),
        );
        update(&mut model, Action::OpenSaveQuery);
        assert_eq!(
            model
                .save_query_prompt
                .as_ref()
                .map(|prompt| prompt.sql.as_str()),
            Some("select 1;")
        );
    }

    /// Esc then `o`, typed fast in Insert mode, reaches Dexo as Alt+O: it leaves Insert
    /// and opens a line, as in Vim, instead of opening the saved queries.
    #[test]
    fn esc_then_a_key_typed_fast_is_two_keys_in_vim() {
        let mut model = Model {
            keymap: crate::keymap::Keymap::vim_profile(),
            focus: Focus::Editor,
            ..Model::default()
        };
        model.set_sql("select 1");
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)),
        );
        assert_eq!(model.vim.mode, crate::screens::vim::Mode::Insert);
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT)),
        );
        assert!(!model.saved_queries.open);
        assert_eq!(model.vim.mode, crate::screens::vim::Mode::Insert);
        assert_eq!(model.active_document().text(), "select 1\n");
    }

    fn saved(name: &str, sql: &str) -> dexo_storage::SavedQuery {
        dexo_storage::SavedQuery {
            id: name.into(),
            connection_id: "c".into(),
            name: name.into(),
            sql: sql.into(),
        }
    }

    /// A refused rename keeps the typed name in the field; a click while renaming opens
    /// nothing; wide names keep the preview's separator in its column.
    #[test]
    fn the_saved_query_picker_keeps_a_refused_rename_and_its_columns() {
        let mut model = Model {
            project_id: "p".into(),
            ..Model::default()
        };
        model.saved_queries.open = true;
        model.saved_queries.set_items(vec![
            saved("日本語の売上", "select 1"),
            saved("orders", "select 2"),
        ]);
        let screen = crate::render::render_to_string(&model, 100, 30);
        let columns: Vec<usize> = screen
            .lines()
            .filter(|line| line.contains("日") || line.contains(" orders"))
            // The rendered text gives a wide character's second cell as a space: cells
            // are characters here.
            .map(|line| line[..line.find(" │ ").unwrap()].chars().count())
            .collect();
        assert_eq!(columns.len(), 2, "{screen}");
        assert_eq!(columns[0], columns[1], "{screen}");
        let key = |model: &mut Model, code| {
            update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
        };
        key(&mut model, KeyCode::F(2));
        model
            .saved_queries
            .renaming
            .as_mut()
            .unwrap()
            .set_text("orders");
        let effects = key(&mut model, KeyCode::Enter);
        assert!(matches!(
            effects.as_slice(),
            [Effect::RenameSavedQuery { .. }]
        ));
        update(
            &mut model,
            Action::SavedQueryDone(Err(
                "a saved query of this connection is already called orders".into(),
            )),
        );
        assert_eq!(
            model
                .saved_queries
                .renaming
                .as_ref()
                .map(|input| input.as_str()),
            Some("orders")
        );
        assert!(model.saved_queries.error.is_some());
    }

    /// A temporary connection is in no table: nothing can be saved for it.
    #[test]
    fn a_temporary_connection_keeps_no_saved_query() {
        let mut model = Model {
            project_id: "p".into(),
            ..Model::default()
        };
        model.set_sql("select 1");
        let temporary = dexo_app::ConnectionProfile::new(
            dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "url",
            "postgres",
            "local",
            serde_json::json!({"host": "h"}),
            dexo_app::connection_profile::SecretRef::new("ref".into()),
        );
        model.connections.temporary = vec![temporary.clone()];
        model.connections.load_profiles(Vec::new());
        model.connection.name = "url".into();
        update(&mut model, Action::OpenSaveQuery);
        assert!(model.save_query_prompt.is_none());
        assert!(
            model
                .messages
                .iter()
                .any(|entry| entry.message.contains("save this one first"))
        );
    }

    /// A save that fails calls off the close waiting on it.
    #[test]
    fn a_failed_save_calls_off_its_close() {
        let mut model = Model::default();
        let document = model.active_document().id.clone();
        model.pending_document_close = Some(crate::model::PendingDocumentClose {
            document: document.clone(),
            revision: 1,
        });
        update(
            &mut model,
            Action::DocumentSaveFailed {
                document,
                message: "disk full".into(),
            },
        );
        assert!(model.pending_document_close.is_none());
    }

    /// A table a run created is known to its own session only -- every session's
    /// generation starts at 1 -- and a DROP takes it away again.
    #[test]
    fn session_tables_stay_with_their_session_and_go_when_dropped() {
        let (a, b) = (
            crate::runtime::SessionId(uuid::Uuid::from_u128(1)),
            crate::runtime::SessionId(uuid::Uuid::from_u128(2)),
        );
        let mut model = Model {
            active_session: Some(a),
            session_generation: 1,
            ..Model::default()
        };
        let run = |model: &mut Model, sql: &str| {
            let effects = super::launch_script(model, vec![sql.into()]);
            let key = effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::StartScript(request) => Some(request.key.clone()),
                    _ => None,
                })
                .unwrap();
            update(model, Action::ScriptFinished { key });
        };
        run(&mut model, "create temp table scratch (id int)");
        assert_eq!(
            crate::screens::editor::session_tables(&model),
            [&"scratch".to_string()]
        );
        model.active_session = Some(b);
        assert!(crate::screens::editor::session_tables(&model).is_empty());
        model.active_session = Some(a);
        run(&mut model, "drop table scratch");
        assert!(crate::screens::editor::session_tables(&model).is_empty());
    }

    /// The startup warning waits for the connection under the name it was opened as.
    #[test]
    fn a_startup_warning_follows_a_clash_rename() {
        let mut model = Model::default();
        let profile = |name: &str| {
            dexo_app::ConnectionProfile::new(
                dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
                None,
                name,
                "postgres",
                "local",
                serde_json::json!({"host": "h"}),
                dexo_app::connection_profile::SecretRef::new("ref".into()),
            )
        };
        model.connections.load_profiles(vec![profile("shop")]);
        super::open_startup_connection(&mut model, profile("shop"), Some("careful".into()));
        assert_eq!(
            model.startup_warning,
            Some(("shop (2)".to_string(), "careful".to_string()))
        );
    }

    /// A copy is never called what an open temporary connection is.
    #[test]
    fn a_copy_avoids_the_temporary_connections_names() {
        let mut model = Model::default();
        let profile = |name: &str| {
            dexo_app::ConnectionProfile::new(
                dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
                None,
                name,
                "postgres",
                "local",
                serde_json::json!({"host": "h"}),
                dexo_app::connection_profile::SecretRef::new("ref".into()),
            )
        };
        model.connections.temporary = vec![profile("pg (copy)")];
        model.connections.load_profiles(vec![profile("pg")]);
        model.connections.selected_profile = 0;
        let effects = update(&mut model, Action::DuplicateConnection);
        assert!(
            matches!(effects.as_slice(), [Effect::DuplicateProfile { taken, .. }] if taken == &["pg (copy)".to_string()]),
            "{effects:?}"
        );
    }

    /// The row's Actions menu says each action's key, and has the sort, the count and
    /// the way back.
    #[test]
    fn the_row_menu_says_its_keys() {
        let mut model = Model {
            focus: Focus::Results,
            ..Model::default()
        };
        model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
            name: "id".into(),
            type_name: "int".into(),
            nullable: false,
        }]);
        model.results.append_rows(vec![vec![DbValue::I64(1)]]);
        model.results.select_row(0);
        update(&mut model, Action::OpenResultsMenu);
        let screen = crate::render::render_to_string(&model, 120, 40);
        let line = |title: &str| {
            screen
                .lines()
                .find(|line| line.contains(title))
                .unwrap_or_else(|| panic!("{title} missing:\n{screen}"))
                .to_string()
        };
        assert!(line("Filter rows (WHERE)").contains(" w "), "{screen}");
        assert!(line("Related rows…").contains(" f "), "{screen}");
        assert!(line("Count rows").contains(" t "), "{screen}");
        line("Sort by this column");
        line("Back from related rows");
    }

    /// Offline, `t` and `f` dial the document's connection and run once it is up.
    #[test]
    fn counting_and_related_rows_connect_by_themselves() {
        for action in [Action::CountRows, Action::OpenRelatedPicker] {
            let mut model = Model::default();
            let profile = dexo_app::ConnectionProfile::new(
                dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
                None,
                "shop",
                "postgres",
                "local",
                serde_json::json!({"host": "h"}),
                dexo_app::connection_profile::SecretRef::new("ref".into()),
            );
            model.connections.load_profiles(vec![profile.clone()]);
            model.documents = vec![crate::model::EditorDocument::new_table(
                dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders"),
                Some(profile.id.0.to_string()),
            )];
            model.active_document = 0;
            let effects = update(&mut model, action.clone());
            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::ConnectProfile { .. })),
                "{action:?}: {effects:?}"
            );
            assert_eq!(
                model
                    .pending_execute
                    .as_ref()
                    .map(|pending| &pending.action),
                Some(&action)
            );
        }
    }

    /// Refreshing the rows lets go of their count, stopping it if it still runs.
    #[test]
    fn a_refresh_lets_go_of_the_count() {
        let mut model = Model {
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            session_generation: 1,
            ..Model::default()
        };
        model.documents = vec![crate::model::EditorDocument::new_table(
            dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders"),
            None,
        )];
        model.active_document = 0;
        super::load_table_document(&mut model, 0);
        let operation = crate::runtime::OperationId::new();
        model.data.count = Some(crate::screens::data::RowCount {
            key: super::count_key(&model).unwrap(),
            state: crate::screens::data::CountState::Running(operation),
        });
        let effects = update(&mut model, Action::RefreshTableData);
        assert!(model.data.count.is_none());
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::CancelCount { operation: o } if *o == operation)
        ));
    }

    /// In the Explain view `i` asks for an index, which is tried on the statement's plan
    /// -- not on the one the cursor moved to since; the plan that comes back is compared
    /// with the one made without it.
    #[test]
    fn an_index_is_tried_against_the_plan_without_it() {
        let session = crate::runtime::SessionId(uuid::Uuid::from_u128(1));
        let mut model = Model {
            active_session: Some(session),
            session_generation: 1,
            focus: Focus::Results,
            ..Model::default()
        };
        let text = "select * from orders where customer_id = 7;\nselect 1;";
        model.set_sql(text);
        let plan = |cost: f64| dexo_driver_api::ExplainPlan {
            planning_ms: None,
            execution_ms: None,
            root: dexo_driver_api::PlanNode {
                kind: if cost > 100.0 {
                    "Seq Scan"
                } else {
                    "Index Scan"
                }
                .into(),
                relation: Some("orders".into()),
                detail: None,
                estimates: dexo_driver_api::PlanMetrics {
                    cost: Some(cost),
                    ..Default::default()
                },
                actual: dexo_driver_api::PlanMetrics::default(),
                loops: None,
                children: Vec::new(),
                native: serde_json::Value::Null,
            },
            raw: String::new(),
        };
        let document = model.active_document().id.clone();
        let sql = "select * from orders where customer_id = 7".to_string();
        let loaded = |model: &mut Model, cost: f64, indexes: Vec<String>| {
            update(
                model,
                Action::ExplainLoaded {
                    plan: Box::new(plan(cost)),
                    sql: sql.clone(),
                    indexes,
                    document: document.clone(),
                    operation: crate::runtime::OperationId::new(),
                },
            );
        };
        loaded(&mut model, 2084.0, Vec::new());
        let end = text.chars().count();
        model.active_document_mut().sql.set_cursor(end).unwrap();
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)),
        );
        let prompt = model.try_index.as_mut().expect("Try index opens");
        prompt
            .input
            .set_text("CREATE INDEX ON orders (customer_id)");
        let effects = update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::RunExplain { sql: explained, cursor, dialect, indexes, analyze: false, .. }
                    if indexes == &["CREATE INDEX ON orders (customer_id)".to_string()]
                        && crate::runtime::explain_manager::statement_sql(explained, *cursor, *dialect)
                            .as_deref() == Some(sql.as_str())
            )),
            "{effects:?}"
        );
        assert!(model.try_index.is_none());
        model.active_operation = None;
        let index = vec!["CREATE INDEX ON orders (customer_id)".to_string()];
        loaded(&mut model, 8.0, index.clone());
        loaded(&mut model, 9.0, index);
        // Both tries are compared with the plan without the index, not with each other.
        let explain = &model.results.explain;
        assert!(!explain.compare.is_empty());
        assert_eq!(
            explain
                .baseline
                .as_ref()
                .and_then(|plan| plan.root.estimates.cost),
            Some(2084.0)
        );
    }

    /// A statement that is not a plain read keeps no statement to run again: no bars,
    /// no sort, no count.
    #[test]
    fn a_result_that_is_not_a_read_keeps_no_statement() {
        let mut model = Model {
            active_session: Some(crate::runtime::SessionId(uuid::Uuid::from_u128(1))),
            ..Model::default()
        };
        super::launch_script(
            &mut model,
            vec!["update t set a = 1".into(), "select 1".into()],
        );
        assert!(model.results.tabs[0].source_sql.is_none());
        assert!(!super::clause_bars_shown(&model));
        assert_eq!(
            model.results.tabs[1].source_sql.as_deref(),
            Some("select 1")
        );
    }

    /// Settings reads the theme files again: a broken one added since is said once, and
    /// a theme in use whose file is gone goes back to Dexo's own, painted at once.
    #[test]
    fn reopened_settings_read_theme_files_again() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        std::fs::create_dir(&themes).unwrap();
        std::fs::write(
            themes.join("mine.toml"),
            "name = \"Mine\"\n[roles]\nbackground = \"#101010\"\n",
        )
        .unwrap();
        let mut model = Model::default();
        model.settings.theme = "file:mine".into();
        super::load_user_themes(&mut model, dir.path());
        assert_eq!(model.theme.name, "Mine");
        std::fs::write(themes.join("broken.toml"), "mode = \"neon\"\n").unwrap();
        super::load_user_themes(&mut model, dir.path());
        super::load_user_themes(&mut model, dir.path());
        let said = |model: &Model, text: &str| {
            model
                .messages
                .iter()
                .filter(|entry| entry.message.contains(text))
                .count()
        };
        assert_eq!(said(&model, "broken.toml line 1"), 1);
        std::fs::remove_file(themes.join("mine.toml")).unwrap();
        super::load_user_themes(&mut model, dir.path());
        assert_eq!(model.settings.theme, crate::theme::DEXO_THEME);
        assert_ne!(model.theme.name, "Mine");
        assert_eq!(said(&model, "There is no theme mine"), 1);
    }

    #[test]
    fn the_theme_has_a_settings_key_and_a_command() {
        let mut model = Model::default();
        model.settings.open = true;
        model.settings.themes = crate::theme::choices(&[]);
        let before = model.settings.theme.clone();
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        );
        assert_ne!(model.settings.theme, before);
        assert!(crate::palette::command_spec("settings.theme").is_some());
    }

    /// a saved connection already dials is not listed again.
    #[test]
    fn docker_rows_without_a_password_or_already_saved() {
        let mut model = Model::default();
        update(&mut model, Action::OpenConnections);
        let found = dexo_app::docker::from_inspect(
            r#"[{"Name": "/open-my", "Config": {"Image": "mysql:8.4", "Env": ["MYSQL_ALLOW_EMPTY_PASSWORD=yes"]},
                 "NetworkSettings": {"Ports": {"3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3309"}]}}},
                {"Name": "/shop-pg", "Config": {"Image": "postgres:16", "Env": ["POSTGRES_PASSWORD=pw"]},
                 "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "0.0.0.0", "HostPort": "5433"}]}}}]"#,
        );
        update(&mut model, Action::DockerDiscovered(found));
        model
            .connections
            .load_profiles(vec![dexo_app::ConnectionProfile::new(
                dexo_app::connection_profile::ConnectionId(uuid::Uuid::new_v4()),
                None,
                "shop",
                "postgres",
                "local",
                serde_json::json!({"host": "127.0.0.1", "port": "5433", "username": "postgres"}),
                dexo_app::connection_profile::SecretRef::new("ref".into()),
            )]);
        let screen = crate::render::render_to_string(&model, 100, 30);
        assert!(screen.contains("open-my [mysql]"), "{screen}");
        assert!(!screen.contains("shop-pg [postgres]"), "{screen}");
        assert!(
            screen.contains("already saved as connections: shop-pg"),
            "{screen}"
        );
        assert_eq!(model.connections.row_count(), 2);
        model.connections.selected_profile = 1;
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        let effects = update(&mut model, Action::SaveConnection);
        assert!(
            matches!(effects.as_slice(), [Effect::CreateConnection { input, password, .. }]
                if password.is_empty() && input.allow_empty_password && input.database == "mysql"),
            "{effects:?}"
        );
    }

    /// Going back to an open session of another driver brings its SQL dialect back.
    #[test]
    fn switching_sessions_switches_the_grid_dialect() {
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

        let mut model = Model::default();
        model.data.dialect = dexo_app::data::SqlDialect::Mysql;
        model.connection.driver = "mysql".into();
        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(3)),
            None,
            "pg",
            "postgres",
            "local",
            serde_json::json!({"host": "db"}),
            SecretRef::new("ref".into()),
        );
        let session = crate::screens::connections::SessionRow {
            id: crate::runtime::SessionId(uuid::Uuid::from_u128(4)),
            connection: "pg".into(),
            transaction: dexo_driver_api::TransactionState::Idle,
            generation: 1,
            environment: "local".into(),
            read_only: false,
            driver: "postgres".into(),
        };
        super::activate_existing_session(&mut model, &profile, session);
        assert_eq!(model.connection.driver, "postgres");
        assert_eq!(model.data.dialect, dexo_app::data::SqlDialect::Postgres);
    }

    /// The secret prompt takes the secret as typed; Enter uses it for the session,
    /// Alt+K first keeps it in the keychain -- never for a temporary connection.
    #[test]
    fn the_secret_prompt_takes_what_is_typed() {
        use crate::screens::secret_prompt::{SecretBuffer, SecretChoiceKind, SecretPurpose};
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};

        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(5)),
            None,
            "shop",
            "postgres",
            "local",
            serde_json::json!({"host": "db"}),
            SecretRef::new("ref".into()),
        );
        let prompt = |model: &mut Model| {
            update(
                model,
                Action::SecretRequired {
                    purpose: SecretPurpose::DatabasePassword,
                    profile: profile.clone(),
                    buffer: SecretBuffer::new(String::new()),
                },
            );
        };
        let key = |code, modifiers| Action::Key(KeyEvent::new(code, modifiers));
        let submitted = |effects: Vec<Effect>| match &effects[..] {
            [Effect::SubmitSecret { kind, secret, .. }] => {
                Some((*kind, secret.expose().to_string()))
            }
            _ => None,
        };

        let mut model = Model::default();
        prompt(&mut model);
        for ch in "pw1".chars() {
            update(&mut model, key(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        update(&mut model, key(KeyCode::Backspace, KeyModifiers::NONE));
        update(&mut model, key(KeyCode::Char('2'), KeyModifiers::NONE));
        assert!(!model.secret_prompt.lines().join("\n").contains("pw2"));
        let effects = update(&mut model, key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            submitted(effects),
            Some((SecretChoiceKind::SessionOnly, "pw2".to_string()))
        );

        prompt(&mut model);
        update(&mut model, key(KeyCode::Char('x'), KeyModifiers::NONE));
        update(&mut model, key(KeyCode::Char('k'), KeyModifiers::ALT));
        let effects = update(&mut model, key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            submitted(effects),
            Some((SecretChoiceKind::SaveToKeychain, "x".to_string()))
        );
    }

    /// A failure the server places becomes an underline there, with the cursor on it.
    #[test]
    fn a_failed_statement_points_at_where_the_server_says() {
        let mut model = Model::default();
        model
            .active_document_mut()
            .sql
            .insert(0, "select 1;\nselect ação, nope from t;")
            .unwrap();
        let effects = super::launch_script(
            &mut model,
            vec!["select 1".into(), "select ação, nope from t".into()],
        );
        let key = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::StartScript(request) => Some(request.key.clone()),
                _ => None,
            })
            .expect("a script");
        update(
            &mut model,
            Action::QueryFailed {
                key,
                index: 1,
                message: "column \"nope\" does not exist".into(),
                details: Vec::new(),
                position: Some(14),
            },
        );
        let text = model.active_document().text();
        let cursor = model.active_document().cursor();
        assert_eq!(
            text.chars().skip(cursor).take(4).collect::<String>(),
            "nope"
        );
        let (_, _, diagnostic) = model.editor.server_diagnostic.clone().expect("underlined");
        assert_eq!(&text[diagnostic.byte_range.unwrap()], "nope");
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
        assert!(
            model.recent_sql_files.is_empty(),
            "a file is a recent one only once it has been read"
        );
        let id = model.documents[1].id.clone();
        update(
            &mut model,
            Action::DocumentLoaded {
                document: id,
                path: path.clone(),
                content: "select 1".into(),
            },
        );
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
        // The columns arrived with the first page: only a table whose never did is
        // opened whole again.
        model.data.table = dexo_app::data::TableMeta {
            columns: vec![dexo_app::data::ColumnDef {
                name: "id".into(),
                primary_key: true,
                unique: true,
                nullable: false,
            }],
        };
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

        // Focus Results is what it says on a table document too: the console under the
        // grid is reached by a click, not by a key named for something else.
        model.focus = Focus::Console;
        update(&mut model, Action::Focus(FocusTarget::Results));
        let view = crate::render::render_to_string(&model, 160, 50);
        assert!(
            view.contains("▸ Results"),
            "Focus Results did not focus the results:\n{view}"
        );
        assert!(
            !view.contains("▸ Console"),
            "the console kept the highlight:\n{view}"
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
        model.results.explain = crate::screens::explain::ExplainScreen::fixture();
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push((model.results.view, model.results.explain.view));
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

    /// With nothing explained there is no plan to stop on: the log is one press away.
    #[test]
    fn output_view_skips_explain_when_there_is_no_plan() {
        use crate::model::ResultsView;

        let mut model = Model::default();
        update(&mut model, Action::CycleResultsView);
        assert_eq!(model.results.view, ResultsView::Messages);
        update(&mut model, Action::CycleResultsView);
        assert_eq!(model.results.view, ResultsView::Grid);
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
                sql: "select 1".into(),
                indexes: Vec::new(),
                document: document.clone(),
                operation: crate::runtime::OperationId::new(),
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
                names: Default::default(),
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

    /// The keychain checkbox is a stop of its own between the secret and the buttons, and a
    /// password the server turned down comes back to the prompt, as typed, with the error.
    #[test]
    fn the_secret_prompt_walks_to_its_checkbox_and_reopens_on_a_rejection() {
        use crate::screens::secret_prompt::{SecretBuffer, SecretPurpose};
        use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::from_u128(6)),
            None,
            "shop",
            "postgres",
            "local",
            serde_json::json!({"host": "db"}),
            SecretRef::new("ref".into()),
        );
        let mut model = Model::default();
        update(
            &mut model,
            Action::SecretRequired {
                purpose: SecretPurpose::DatabasePassword,
                profile: profile.clone(),
                buffer: SecretBuffer::new(String::new()),
            },
        );
        let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
        update(&mut model, key(KeyCode::Tab));
        assert!(
            model.secret_prompt.keychain_focus,
            "Tab reaches the checkbox"
        );
        update(&mut model, key(KeyCode::Char(' ')));
        assert!(model.secret_prompt.keychain, "Space flips it");
        update(&mut model, key(KeyCode::Tab));
        assert_eq!(
            model.secret_prompt.footer,
            crate::widgets::form::FooterFocus::Submit
        );
        assert!(!model.secret_prompt.keychain_focus);

        update(
            &mut model,
            Action::SecretRejected {
                purpose: SecretPurpose::DatabasePassword,
                profile,
                buffer: SecretBuffer::new("typo"),
                keychain: true,
                message: "password authentication failed for user \"dexo\"".into(),
            },
        );
        assert!(model.secret_prompt.open);
        assert_eq!(model.secret_prompt.buffer.expose(), "typo");
        assert!(model.secret_prompt.keychain);
        let lines = model.secret_prompt.lines().join("\n");
        assert!(lines.contains("password authentication failed"), "{lines}");
    }
}
