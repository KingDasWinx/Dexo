use dexo_tui::action::Action;
use dexo_tui::model::Model;
use dexo_tui::palette::palette_entries;
use dexo_tui::render::render_to_string;
use dexo_tui::update::update;

#[test]
fn snapshot_settings_full_and_compact() {
    let mut model = Model::default();
    update(&mut model, Action::OpenSettings);
    model.settings = dexo_tui::screens::settings::SettingsScreen::fixture();
    insta::assert_snapshot!("settings_full", render_to_string(&model, 160, 50));
    insta::assert_snapshot!("settings_compact", render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_recovery_full_and_compact() {
    let mut model = Model::default();
    update(&mut model, Action::OpenRecovery);
    model.recovery = dexo_tui::screens::recovery::RecoveryScreen::fixture();
    assert!(model.recovery.lines().join("\n").contains("unknown"));
    assert!(!model.recovery.lines().join("\n").contains("active"));
    insta::assert_snapshot!("recovery_full", render_to_string(&model, 160, 50));
    insta::assert_snapshot!("recovery_compact", render_to_string(&model, 60, 20));
}

#[test]
fn snapshot_mcp_audit_and_revoke() {
    let mut model = Model::default();
    update(&mut model, Action::OpenMcpProfiles);
    model.mcp_profiles = dexo_tui::screens::mcp_profiles::McpProfilesScreen::fixture();
    update(&mut model, Action::OpenMcpAudit);
    model.mcp_audit = dexo_tui::screens::mcp_audit::McpAuditScreen::fixture();
    insta::assert_snapshot!("mcp_audit_full", render_to_string(&model, 160, 50));
    update(&mut model, Action::RevokeAllMcpGrants);
    update(&mut model, Action::RevokeAllMcpGrants);
    insta::assert_snapshot!("mcp_revoke_all", render_to_string(&model, 160, 50));
    model.capabilities.unicode = false;
    model.capabilities.color_depth = dexo_tui::capabilities::ColorDepth::None;
    insta::assert_snapshot!("mcp_nocolor", render_to_string(&model, 100, 30));
}

#[test]
fn action_registry_every_command_is_palette_reachable() {
    let registered: std::collections::BTreeSet<_> = palette_entries(&Model::default())
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(registered.len(), palette_entries(&Model::default()).len());
}

#[test]
fn keyboard_only_opens_settings_and_recovery() {
    let mut model = Model {
        mouse: false,
        ..Model::default()
    };
    update(
        &mut model,
        Action::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }),
    );
    assert!(!model.settings.open);
    update(&mut model, Action::OpenSettings);
    assert!(model.settings.open);
    update(&mut model, Action::OpenRecovery);
    assert!(model.recovery.open);
}
