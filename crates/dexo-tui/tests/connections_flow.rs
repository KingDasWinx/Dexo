use dexo_app::{
    ConnectionId, ConnectionPolicyOverrides, ConnectionProfile, Project, ProjectId, SecretRef,
};
use dexo_secrets::{SecretError, SecretStore};
use dexo_storage::{RecoveryDocument, SessionRecoveryState};
use dexo_tui::action::{Action, Effect};
use dexo_tui::model::Model;
use dexo_tui::runtime::connection_manager::connect_with_store;
use dexo_tui::runtime::{SessionId, storage_worker::BootstrapState};
use dexo_tui::screens::secret_prompt::{
    DeleteSecretDecision, SecretBuffer, SecretChoiceKind, SecretPurpose,
};
use dexo_tui::update;
use secrecy::SecretString;

struct UnavailableSecretStore;

impl SecretStore for UnavailableSecretStore {
    fn put(&self, _key: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }

    fn get(&self, _key: &str) -> Result<Option<SecretString>, SecretError> {
        Err(SecretError::Unavailable)
    }

    fn delete(&self, _key: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
}

fn saved_profile() -> ConnectionProfile {
    ConnectionProfile::new(
        ConnectionId(uuid::Uuid::nil()),
        None,
        "prod",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","port":5432,"username":"u","database":"d"}),
        SecretRef::new("ref-1".into()),
    )
}

fn custom_policy_profile() -> ConnectionProfile {
    let mut profile = saved_profile();
    profile.name = "pci-lab".into();
    profile.environment = "pci-lab".into();
    profile.policy = ConnectionPolicyOverrides {
        read_only: Some(true),
        confirm_destructive: Some(true),
        require_verified_tls: Some(true),
        max_rows: Some(10),
        timeout_secs: Some(3),
    };
    profile
}

#[test]
fn locked_keychain_prompts_instead_of_silently_using_memory() {
    let profile = saved_profile();
    let action = *connect_with_store(&UnavailableSecretStore, &profile).unwrap_err();
    assert!(matches!(
        action,
        Action::SecretRequired {
            purpose: SecretPurpose::DatabasePassword,
            ..
        }
    ));
}

#[test]
fn secret_buffer_debug_is_redacted() {
    let buffer = SecretBuffer::new("SUPER_SECRET_SENTINEL");
    assert_eq!(format!("{buffer:?}"), "SecretBuffer([REDACTED])");
}

#[test]
fn bootstrap_lists_profiles_without_auto_connecting() {
    let mut model = Model::default();
    let bootstrap = BootstrapState {
        active_project: Project {
            id: ProjectId(uuid::Uuid::nil()),
            name: "Default".into(),
            created_at: "now".into(),
        },
        connections: vec![saved_profile()],
        recovery: SessionRecoveryState {
            clean_shutdown: true,
            layout: None,
            documents: Vec::new(),
            transaction: "idle".into(),
        },
        layout: None,
        documents: Vec::new(),
        projects: Vec::new(),
        snippets: Vec::new(),
    };
    let _ = update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));
    assert_eq!(model.connections.profiles.len(), 1);
    assert!(!model.connection.ready);
}


#[test]
fn bootstrap_restores_checkpoints_automatically_without_a_prompt() {
    let mut model = Model::default();
    let bootstrap = BootstrapState {
        active_project: Project {
            id: ProjectId(uuid::Uuid::nil()),
            name: "Default".into(),
            created_at: "now".into(),
        },
        connections: Vec::new(),
        recovery: SessionRecoveryState {
            clean_shutdown: true,
            layout: None,
            documents: vec![RecoveryDocument {
                id: "scratch".into(),
                project_id: uuid::Uuid::nil().to_string(),
                title: "scratch.sql".into(),
                content: "select 42".into(),
                updated_at: "now".into(),
            }],
            transaction: "idle".into(),
        },
        layout: None,
        documents: Vec::new(),
        projects: Vec::new(),
        snippets: Vec::new(),
    };

    let _ = update(&mut model, Action::Bootstrapped(Box::new(bootstrap)));

    assert!(!model.recovery.open);
    assert_eq!(model.documents.len(), 1);
    assert_eq!(model.documents[0].text(), "select 42");
}
#[test]
fn secret_required_opens_prompt() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Action::SecretRequired {
            purpose: SecretPurpose::DatabasePassword,
            profile: saved_profile(),
            buffer: SecretBuffer::new("x"),
        },
    );
    assert!(model.secret_prompt.open);
    assert_eq!(model.secret_prompt.profile_name, "prod");
}

#[test]
fn submit_secret_session_only_emits_persist_effect() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Action::SecretRequired {
            purpose: SecretPurpose::DatabasePassword,
            profile: saved_profile(),
            buffer: SecretBuffer::new("pw"),
        },
    );
    let effects = update(
        &mut model,
        Action::SubmitSecret {
            kind: SecretChoiceKind::SessionOnly,
        },
    );
    assert!(!model.secret_prompt.open);
    assert!(matches!(
        effects.as_slice(),
        [Effect::SubmitSecret {
            kind: SecretChoiceKind::SessionOnly,
            ..
        }]
    ));
}

#[test]
fn delete_prompts_keep_or_delete_secrets() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    let _ = update(&mut model, Action::DeleteConnection);
    assert_eq!(
        model
            .connections
            .delete_target
            .as_ref()
            .map(|p| p.name.as_str()),
        Some("prod")
    );
    let effects = update(
        &mut model,
        Action::ConfirmDeleteProfile {
            decision: DeleteSecretDecision::KeepSecrets,
        },
    );
    assert!(model.connections.delete_target.is_none());
    assert!(matches!(
        effects.as_slice(),
        [Effect::DeleteProfile {
            delete_secrets: false,
            ..
        }]
    ));
}

#[test]
fn connect_and_switch_emit_effects_only() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    let effects = update(&mut model, Action::ConnectSelected);
    assert!(matches!(
        effects.as_slice(),
        [Effect::ConnectProfile { token: 1, .. }]
    ));
    assert!(!model.connection.ready);
}

#[test]
fn duplicate_test_delete_and_group_move_are_effects() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    assert!(matches!(
        update(&mut model, Action::DuplicateConnection).as_slice(),
        [Effect::DuplicateProfile { .. }]
    ));
    assert!(matches!(
        update(&mut model, Action::TestConnection).as_slice(),
        [Effect::TestSavedProfile { .. }]
    ));
    assert!(matches!(
        update(
            &mut model,
            Action::MoveConnectionGroup {
                group: "prod/east".into()
            }
        )
        .as_slice(),
        [Effect::MoveProfileGroup { .. }]
    ));
}

#[test]
fn profile_saved_and_deleted_update_the_browser() {
    let mut model = Model::default();
    let _ = update(&mut model, Action::ProfileSaved(saved_profile()));
    assert_eq!(model.connections.profiles.len(), 1);
    let _ = update(
        &mut model,
        Action::ProfileDeleted {
            name: "prod".into(),
        },
    );
    assert!(model.connections.profiles.is_empty());
}

#[test]
fn one_session_per_connection_and_switch() {
    let mut model = Model::default();
    let staging = {
        let mut profile = saved_profile();
        profile.name = "staging".into();
        profile.id = ConnectionId(uuid::Uuid::from_u128(2));
        profile
    };
    model
        .connections
        .load_profiles(vec![saved_profile(), staging]);
    let prod = SessionId(uuid::Uuid::from_u128(1));
    let staging_session = SessionId(uuid::Uuid::from_u128(2));
    let _ = update(
        &mut model,
        Action::ConnectionChanged {
            name: "prod".into(),
            ready: true,
            environment: "local".into(),
            session: Some(prod),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    let _ = update(
        &mut model,
        Action::ConnectionChanged {
            name: "prod".into(),
            ready: true,
            environment: "local".into(),
            session: Some(SessionId(uuid::Uuid::from_u128(9))),
            generation: 2,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    assert_eq!(model.connections.sessions.len(), 1);
    assert_eq!(model.connections.profiles[0].sessions, 1);
    let _ = update(
        &mut model,
        Action::ConnectionChanged {
            name: "staging".into(),
            ready: true,
            environment: "local".into(),
            session: Some(staging_session),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    assert_eq!(model.active_session, Some(staging_session));
    model.connections.selected_profile = 0;
    let effects = update(&mut model, Action::ConnectSelected);
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadCatalogChildren { .. }]
    ));
    assert_eq!(model.connection.name, "prod");
    assert_eq!(model.connections.sessions.len(), 2);
    assert!(update(&mut model, Action::ConnectSelected).is_empty());
}

#[test]
fn stale_connect_completion_is_ignored() {
    let mut model = Model::default();
    model.connections.load_profiles(vec![saved_profile()]);
    let _ = update(&mut model, Action::ConnectSelected);
    model.connect_token = 2;
    model.connections.pending_connect = Some(2);
    let _ = update(
        &mut model,
        Action::ConnectionChanged {
            name: "prod".into(),
            ready: true,
            environment: "local".into(),
            session: Some(SessionId(uuid::Uuid::from_u128(9))),
            generation: 1,
            token: 1,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    assert!(!model.connection.ready);
}

#[test]
fn read_only_blocks_begin_transaction() {
    let mut model = Model {
        active_session: Some(SessionId(uuid::Uuid::nil())),
        connection: dexo_tui::model::ConnectionStatus {
            read_only: true,
            ..Default::default()
        },
        ..Model::default()
    };
    let effects = update(&mut model, Action::BeginTransaction);
    assert!(effects.is_empty());
    assert!(model.messages.iter().any(|m| m.contains("read-only")));
}

#[test]
fn custom_environment_policy_is_visible_on_the_row() {
    let mut model = Model::default();
    model
        .connections
        .load_profiles(vec![custom_policy_profile()]);
    let lines = model.connections.lines(None).join("\n");
    assert!(lines.contains("pci-lab"));
    assert!(lines.contains(" ro"));
}

#[test]
fn connection_tested_does_not_auto_connect() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Action::ConnectionTested {
            name: "prod".into(),
            ok: true,
            message: "ok".into(),
        },
    );
    assert!(!model.connection.ready);
}

#[test]
fn closing_active_session_clears_live_explorer() {
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

    let mut model = Model::default();
    let session = SessionId(uuid::Uuid::from_u128(7));
    model.connection.name = "prod".into();
    model.connection.ready = true;
    model.active_session = Some(session);
    model.session_generation = 3;
    model.connections.upsert_session(dexo_tui::screens::connections::SessionRow {
        id: session,
        connection: "prod".into(),
        transaction: dexo_driver_api::TransactionState::Idle,
        generation: 3,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("catalog:db"),
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), Some("db"), "db"),
            None,
        )],
        restrictions: vec![],
    });
    assert!(!model.explorer.roots.is_empty());

    let effects = update(&mut model, Action::SessionClosed { session });

    assert!(model.active_session.is_none());
    assert!(!model.connection.ready);
    assert!(model.explorer.roots.is_empty());
    assert!(model.explorer.offline);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadOfflineCatalog { .. })),
        "{effects:?}"
    );
}

#[test]
fn deleting_active_connection_clears_explorer() {
    use dexo_driver_api::{CatalogList, CatalogObject, ObjectId, ObjectKind, QualifiedName};

    let mut model = Model::default();
    let session = SessionId(uuid::Uuid::from_u128(8));
    model.connections.load_profiles(vec![saved_profile()]);
    model.connection.name = "prod".into();
    model.connection.ready = true;
    model.active_session = Some(session);
    model.connections.upsert_session(dexo_tui::screens::connections::SessionRow {
        id: session,
        connection: "prod".into(),
        transaction: dexo_driver_api::TransactionState::Idle,
        generation: 1,
        environment: "local".into(),
        read_only: false,
        driver: "postgres".into(),
    });
    model.explorer.replace_roots(CatalogList {
        objects: vec![CatalogObject::new(
            ObjectId::new("catalog:db"),
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), Some("db"), "db"),
            None,
        )],
        restrictions: vec![],
    });

    let effects = update(
        &mut model,
        Action::ProfileDeleted {
            name: "prod".into(),
        },
    );

    assert!(model.connections.profiles.is_empty());
    assert!(model.connections.sessions.is_empty());
    assert!(model.active_session.is_none());
    assert!(!model.connection.ready);
    assert!(model.connection.name.is_empty());
    assert!(model.explorer.roots.is_empty());
    assert!(!model.explorer.offline);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CloseSession { .. })),
        "{effects:?}"
    );
}
