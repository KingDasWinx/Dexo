use dexo_driver_api::DbValue;
use dexo_tui::action::Action;
use dexo_tui::model::{Model, OperationStatus, ResultKey, ResultTab};
use dexo_tui::runtime::{OperationId, OperationKey};
use dexo_tui::update;
use uuid::Uuid;

fn op_key() -> OperationKey {
    OperationKey::new(OperationId(Uuid::from_u128(1)), "", "scratch", 1)
}

fn result_key(index: usize) -> ResultKey {
    ResultKey {
        operation: op_key(),
        index,
    }
}

fn rows_action(key: ResultKey, rows: Vec<Vec<DbValue>>) -> Action {
    Action::QueryRows {
        key: key.operation,
        index: key.index,
        rows,
    }
}

fn model_with_two_running_results() -> Model {
    let mut model = Model {
        session_generation: 1,
        ..Model::default()
    };
    let key = op_key();
    model.results.tabs = vec![
        {
            let mut tab = ResultTab::new(result_key(0), "r0");
            tab.status = OperationStatus::Running;
            tab
        },
        {
            let mut tab = ResultTab::new(result_key(1), "r1");
            tab.status = OperationStatus::Running;
            tab
        },
    ];
    model.active_operation = Some(key.operation);
    model
}

#[test]
fn batches_update_only_the_correlated_result_set() {
    let mut model = model_with_two_running_results();
    update(
        &mut model,
        rows_action(result_key(1), vec![vec![DbValue::I64(2)]]),
    );
    assert_eq!(model.results.tabs[0].grid.row_count(), 0);
    assert_eq!(model.results.tabs[1].grid.row_count(), 1);
}

#[test]
fn paging_applies_only_matching_generation() {
    let mut model = Model {
        session_generation: 2,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        ..Model::default()
    };
    update(
        &mut model,
        Action::DataPageLoaded {
            generation: 1,
            session: Uuid::from_u128(1).to_string(),
            page: dexo_driver_api::DataPage::from_fetched(
                vec![dexo_driver_api::ColumnMeta {
                    name: "id".into(),
                    type_name: "int".into(),
                    nullable: false,
                }],
                vec![vec![DbValue::I64(1)]],
                0,
                50,
            ),
        },
    );
    assert_eq!(model.results.row_count(), 0);
    update(
        &mut model,
        Action::DataPageLoaded {
            generation: 2,
            session: Uuid::from_u128(1).to_string(),
            page: dexo_driver_api::DataPage::from_fetched(
                vec![dexo_driver_api::ColumnMeta {
                    name: "id".into(),
                    type_name: "int".into(),
                    nullable: false,
                }],
                vec![vec![DbValue::I64(9)]],
                100,
                50,
            ),
        },
    );
    assert_eq!(model.results.row_count(), 1);
    assert_eq!(model.data.page_offset, 100);
}

#[test]
fn clipboard_copy_emits_os_effect() {
    let mut model = Model::default();
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![DbValue::I64(1)]]);
    let effects = update(
        &mut model,
        Action::CopyGrid(dexo_app::data::CopyFormat::Csv),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::CopyToClipboard { .. }))
    );
    assert!(model.data.clipboard.is_empty());
}

#[test]
fn clipboard_failure_is_not_success() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::ClipboardFailed {
            message: "denied".into(),
        },
    );
    assert!(model.data.clipboard.is_empty());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.contains("denied"))
    );
    update(
        &mut model,
        Action::ClipboardWritten {
            text: "id\n1\n".into(),
        },
    );
    assert_eq!(model.data.clipboard, "id\n1\n");
}

#[test]
fn clipboard_formats_cover_cell_row_column_and_range() {
    let mut model = Model::default();
    model.results.set_columns(vec![
        dexo_driver_api::ColumnMeta {
            name: "id".into(),
            type_name: "int".into(),
            nullable: false,
        },
        dexo_driver_api::ColumnMeta {
            name: "n".into(),
            type_name: "text".into(),
            nullable: true,
        },
    ]);
    model.results.append_rows(vec![
        vec![DbValue::I64(1), DbValue::Null],
        vec![DbValue::I64(2), DbValue::Text(String::new())],
    ]);
    model.results.select_cell(0, 0);
    for format in [
        dexo_app::data::CopyFormat::Csv,
        dexo_app::data::CopyFormat::Tsv,
        dexo_app::data::CopyFormat::Json,
        dexo_app::data::CopyFormat::Markdown,
        dexo_app::data::CopyFormat::Sql,
    ] {
        let effects = update(&mut model, Action::CopyGrid(format));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, dexo_tui::Effect::CopyToClipboard { .. })),
            "{format:?}"
        );
    }
    model.results.select_row(1);
    assert!(
        !update(
            &mut model,
            Action::CopyGrid(dexo_app::data::CopyFormat::Csv)
        )
        .is_empty()
    );
    model.results.select_column(1);
    assert!(
        !update(
            &mut model,
            Action::CopyGrid(dexo_app::data::CopyFormat::Tsv)
        )
        .is_empty()
    );
    model.results.select_range((0, 0), (1, 1));
    assert!(
        !update(
            &mut model,
            Action::CopyGrid(dexo_app::data::CopyFormat::Json)
        )
        .is_empty()
    );
}

#[test]
fn foreign_key_null_disables_navigation() {
    let mut model = Model::default();
    model.data.related_fk = Some(dexo_app::data::ForeignKey {
        local: vec!["user_id".into()],
        referenced_table: dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "users"),
        referenced: vec!["id".into()],
    });
    model.data.related_row = vec![("user_id".into(), None)];
    let effects = update(&mut model, Action::OpenRelated);
    assert!(effects.is_empty());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.contains("null"))
    );
}

#[test]
fn arbitrary_select_rewrite_rejects_updates() {
    use dexo_driver_api::Page;
    assert!(
        dexo_sql::derive_page(
            "update users set name='x'",
            &[],
            &None,
            Page::new(0, 10).unwrap()
        )
        .is_err()
    );
}

#[test]
fn arbitrary_select_marks_unsupported_tabs_local_only() {
    let mut model = Model::default();
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("update users set name='x'".into());
    model.results.tabs = vec![tab];
    let effects = update(&mut model, Action::ApplyRemoteSort);
    assert!(effects.is_empty());
    assert!(
        model.results.tabs[0]
            .local_only
            .as_ref()
            .is_some_and(|reason| reason.contains("read-only"))
    );
}

#[test]
fn arbitrary_select_emits_derived_script() {
    let mut model = Model::default();
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id,name from users".into());
    model.results.tabs = vec![tab];
    let effects = update(&mut model, Action::ApplyRemoteSort);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        dexo_tui::Effect::StartScript(request) if request.statements[0].contains("_dexo_derived")
    )));
    assert!(model.results.tabs[0].local_only.is_none());
}

#[test]
fn large_value_grid_never_owns_blob() {
    let mut model = Model::default();
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "blob".into(),
        type_name: "bytea".into(),
        nullable: true,
    }]);
    let total = 40 * 1024 * 1024;
    model
        .results
        .append_rows(vec![vec![DbValue::Bytes(vec![7u8; total])]]);
    assert!(model.results.estimated_bytes() < 1024 * 1024);
    assert!(matches!(
        model.results.cell_at(0, 0),
        Some(dexo_tui::GridCell::Spool { total: stored, .. }) if *stored == total as u64
    ));
    model.results.clear();
    assert!(model.results.cell_at(0, 0).is_none());
}

#[test]
fn large_value_cancel_deletes_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = dexo_tui::runtime::result_spool::spool_bytes(dir.path(), b"payload").unwrap();
    assert!(file.path.exists());
    dexo_tui::runtime::result_spool::delete_spool(&file);
    assert!(!file.path.exists());
    let tmp = dir.path().join("partial.tmp");
    std::fs::write(&tmp, b"partial").unwrap();
    dexo_tui::runtime::result_spool::delete_partial(dir.path());
    assert!(!tmp.exists());
}

#[test]
fn apply_changes_emits_mutations_and_conflict_keeps_edits() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    model.data.table = dexo_app::data::TableMeta {
        columns: vec![dexo_app::data::ColumnDef {
            name: "id".into(),
            primary_key: true,
            unique: true,
            nullable: false,
        }],
    };
    model.data.changes = dexo_app::data::ChangeSet::for_table(&model.data.table);
    model.data.target = dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "items");
    model
        .data
        .changes
        .insert(vec![("id".into(), DbValue::I64(1))]);
    update(&mut model, Action::OpenReview);
    let effects = update(&mut model, Action::ApplyChanges);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::ApplyMutations { .. }))
    );
    update(
        &mut model,
        Action::MutationsFailed {
            generation: 1,
            message: "mutation conflict".into(),
        },
    );
    assert_eq!(model.data.changes.pending().len(), 1);
    assert!(model.data.failed_still_editable());
}

#[test]
fn foreign_key_composite_loads_destination() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    model.data.related_fk = Some(dexo_app::data::ForeignKey {
        local: vec!["org_id".into(), "user_id".into()],
        referenced_table: dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), "users"),
        referenced: vec!["org".into(), "id".into()],
    });
    model.data.related_row = vec![
        ("org_id".into(), Some(DbValue::I64(7))),
        ("user_id".into(), Some(DbValue::I64(3))),
    ];
    let effects = update(&mut model, Action::OpenRelated);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        dexo_tui::Effect::LoadTableData { request, .. }
            if matches!(request.filter, Some(dexo_driver_api::Filter::And(ref parts)) if parts.len() == 2)
    )));
    assert_eq!(model.data.crumbs.len(), 1);
    update(&mut model, Action::DataNavBack);
    assert!(model.data.crumbs.is_empty());
}
