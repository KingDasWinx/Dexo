use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dexo_app::data::{inspect_value, related_filter};
use dexo_driver_api::{DbValue, QueryRequest, TransactionState};

use crate::action::{Action, Effect, FocusTarget};
use crate::model::{Focus, Model};

pub fn update(model: &mut Model, action: Action) -> Vec<Effect> {
    match action {
        Action::Key(key) => handle_key(model, key),
        Action::Mouse(_) if !model.mouse => Vec::new(),
        Action::Mouse(_) => Vec::new(),
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
                    });
                model.connections.selected_session = Some(id);
            }
            model.explorer.clear();
            if ready {
                if let Some(session) = session {
                    let operation = crate::runtime::OperationId::new();
                    return vec![Effect::LoadCatalogChildren {
                        parent: None,
                        operation,
                        session,
                        generation,
                        replace_roots: true,
                        include_system: model.explorer.include_system,
                    }];
                }
            } else {
                model.explorer.offline = true;
                return vec![Effect::LoadOfflineCatalog {
                    connection_id: model.connection.name.clone(),
                    database_name: catalog_database(model),
                    generation,
                }];
            }
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
            .selected_session
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
            model.messages.push(format!("deleted {name}"));
            Vec::new()
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
                model.active_session = model.connections.selected_session;
                if model.active_session.is_none() {
                    model.connection.ready = false;
                }
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
        Action::ExecuteQuery => start_query(model),
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
            crate::screens::editor::end_typing(model);
            let leaving_editor =
                model.focus == Focus::Editor && !matches!(target, FocusTarget::Editor);
            model.focus = match target {
                FocusTarget::Explorer => Focus::Explorer,
                FocusTarget::Editor => Focus::Editor,
                FocusTarget::Results => Focus::Results,
                FocusTarget::Inspector => Focus::Inspector,
            };
            close_palette(model);
            if leaving_editor {
                checkpoint_dirty(model)
            } else {
                Vec::new()
            }
        }
        Action::ExplorerExpand => expand_selected_catalog(model),
        Action::RefreshCatalogNode => refresh_catalog(model, false),
        Action::RefreshCatalogSubtree | Action::RefreshCatalogAll => refresh_catalog(model, true),
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
        Action::OpenObjectData => {
            model.messages.push("data tabs require Sprint 20".into());
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
            model.explorer.copied = Some(text);
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
            let sql = model.active_document().text();
            if !sql.trim().is_empty() {
                model.schema_editor.apply_raw(sql);
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
        Action::SubmitParameters => {
            crate::screens::editor::submit_parameters(model);
            start_query(model)
        }
        Action::SearchHistory => vec![Effect::LoadHistory {
            connection_id: None,
        }],
        Action::ClearHistory => vec![Effect::ClearHistory {
            connection_id: model.connection.name.clone(),
        }],
        Action::HistoryLoaded(entries) => {
            model.editor.history = entries;
            Vec::new()
        }
        Action::SnippetsLoaded(snippets) => {
            model.editor.snippets = snippets;
            Vec::new()
        }
        Action::DocumentLoaded { document, content } => {
            if let Some(doc) = model.documents.iter_mut().find(|item| item.id == document) {
                *doc = crate::model::EditorDocument::with_text(content);
                doc.id = document;
            }
            Vec::new()
        }
        Action::DocumentConflict { path } => {
            model.messages.push(format!("file changed on disk: {path}"));
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
            let height = model.results.viewport().height as i32;
            model.results.scroll_rows(-height);
            Vec::new()
        }
        Action::ResultsPageDown => {
            let height = model.results.viewport().height as i32;
            model.results.scroll_rows(height);
            Vec::new()
        }
        Action::ResultsTop => {
            let offset = model.results.viewport().row_offset as i32;
            model.results.scroll_rows(-offset);
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

fn handle_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    if key.kind != KeyEventKind::Press {
        return Vec::new();
    }
    if model.palette.open {
        return handle_palette_key(model, key);
    }
    if model.secret_prompt.open {
        return handle_secret_prompt_key(model, key);
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
    if model.tabs.active != 2 && crate::screens::editor::handle_key(model, key) {
        crate::screens::editor::refresh_intelligence(model, false);
        return Vec::new();
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
            Vec::new()
        }
        KeyCode::Enter => connect_selected(model),
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
        KeyCode::Char('e') => {
            if let Some(profile) = model.connections.selected().cloned() {
                model.connection_form =
                    crate::screens::connection::ConnectionForm::open_edit(&profile);
            }
            Vec::new()
        }
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

fn connect_selected(model: &mut Model) -> Vec<Effect> {
    let Some(profile) = model.connections.selected().cloned() else {
        return Vec::new();
    };
    model.connect_token = model.connect_token.saturating_add(1);
    model.connections.pending_connect = Some(model.connect_token);
    vec![Effect::ConnectProfile {
        profile,
        token: model.connect_token,
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
        .map(|document| {
            Effect::CheckpointRecovery(crate::action::RecoveryCheckpointRequest {
                document: document.id.clone(),
                project_id: model.project_id.clone(),
                title: document.title.clone(),
                content: document.text(),
            })
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
        .map(|(index, _)| crate::model::ResultTab {
            key: crate::model::ResultKey {
                operation: key.clone(),
                index,
            },
            title: format!("result {}", index + 1),
            grid: crate::model::GridModel::default(),
            status: crate::model::OperationStatus::Idle,
            rows_affected: None,
            notices: Vec::new(),
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
        model.recovery.documents = state
            .recovery
            .documents
            .into_iter()
            .map(|document| document.title)
            .collect();
    }
    model.connections.load_profiles(state.connections);
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
        model.results.tabs.push(crate::model::ResultTab {
            key: crate::model::ResultKey {
                operation: key.clone(),
                index: next,
            },
            title: format!("result {}", next + 1),
            grid: crate::model::GridModel::default(),
            status: crate::model::OperationStatus::Running,
            rows_affected: None,
            notices: Vec::new(),
        });
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

fn refresh_catalog(model: &mut Model, all: bool) -> Vec<Effect> {
    let operation = crate::runtime::OperationId::new();
    if all || model.explorer.selected.is_none() {
        model.explorer.clear();
        return catalog_load_effect(model, None, operation, true);
    }
    let Some(id) = model.explorer.selected.clone() else {
        return Vec::new();
    };
    model.explorer.expand_with(&id, operation);
    catalog_load_effect(model, Some(id), operation, false)
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
    model.results.tabs.push(crate::model::ResultTab {
        key: crate::model::ResultKey {
            operation: crate::runtime::OperationKey::new(
                crate::runtime::OperationId::new(),
                String::new(),
                String::new(),
                model.session_generation,
            ),
            index: model.results.tabs.len(),
        },
        title: title.clone(),
        grid: crate::model::GridModel::default(),
        status: crate::model::OperationStatus::Idle,
        rows_affected: None,
        notices: Vec::new(),
    });
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
                Vec::new()
            }
            KeyCode::Enter => {
                let name = std::mem::take(&mut model.projects.name_input);
                let create = model.projects.mode == crate::screens::projects::ProjectsMode::Create;
                model.projects.mode = crate::screens::projects::ProjectsMode::Browse;
                if create {
                    update(model, Action::CreateProject { name })
                } else {
                    update(model, Action::RenameProject { name })
                }
            }
            KeyCode::Backspace => {
                model.projects.name_input.pop();
                Vec::new()
            }
            KeyCode::Char(ch) => {
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
                Vec::new()
            }
            KeyCode::Enter => {
                let name = model
                    .projects
                    .selected()
                    .map(|project| project.name.clone())
                    .unwrap_or_default();
                update(model, Action::SwitchProject { name })
            }
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
                Vec::new()
            }
            KeyCode::Char('r') => {
                model.projects.mode = crate::screens::projects::ProjectsMode::Rename;
                model.projects.name_input = model
                    .projects
                    .selected()
                    .map(|project| project.name.clone())
                    .unwrap_or_default();
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
}
