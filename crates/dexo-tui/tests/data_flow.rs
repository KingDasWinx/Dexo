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
    let ticket = dexo_tui::runtime::OperationId::new();
    model.data.page_ticket = Some(ticket);
    update(
        &mut model,
        Action::DataPageLoaded {
            generation: 1,
            session: Uuid::from_u128(1).to_string(),
            ticket,
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
            ticket,
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
            .any(|message| message.message.contains("denied"))
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
    model.data.related_row = vec![("user_id".into(), None)];
    let effects = follow(
        &mut model,
        dexo_app::data::ForeignKey {
            local: vec!["user_id".into()],
            referenced_table: dexo_driver_api::QualifiedName::new(
                Some("db"),
                Some("public"),
                "users",
            ),
            referenced: vec!["id".into()],
        },
    );
    assert!(effects.is_empty());
    assert!(
        model
            .messages
            .iter()
            .any(|message| message.message.contains("is NULL"))
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
    let effects = apply_bars(&mut model);
    assert!(effects.is_empty());
    assert!(
        model.results.tabs[0]
            .local_only
            .as_ref()
            .is_some_and(|reason| reason.contains("read-only"))
    );
}

/// Sorting re-runs the statement behind the grid. One that is not a plain read --
/// a side-effecting function, a locking read -- is sorted locally instead of run again.
#[test]
fn sorting_never_runs_a_statement_that_is_not_a_read_again() {
    for sql in [
        "select pg_terminate_backend(42)",
        "select * from users for update",
    ] {
        let mut model = Model::default();
        let mut tab = ResultTab::new(result_key(0), "r0");
        tab.source_sql = Some(sql.into());
        model.results.tabs = vec![tab];
        let effects = apply_bars(&mut model);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, dexo_tui::Effect::StartScript(_))),
            "{sql} ran again"
        );
        assert!(model.results.tabs[0].local_only.is_some(), "{sql}");
    }
}

#[test]
fn arbitrary_select_emits_derived_script() {
    let mut model = Model::default();
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id,name from users".into());
    model.results.tabs = vec![tab];
    let effects = apply_bars(&mut model);
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
    model.data.related_row = vec![
        ("org_id".into(), Some(DbValue::I64(7))),
        ("user_id".into(), Some(DbValue::I64(3))),
    ];
    let before = model.documents.len();
    let effects = follow(
        &mut model,
        dexo_app::data::ForeignKey {
            local: vec!["org_id".into(), "user_id".into()],
            referenced_table: dexo_driver_api::QualifiedName::new(
                Some("db"),
                Some("public"),
                "users",
            ),
            referenced: vec!["org".into(), "id".into()],
        },
    );
    assert!(effects.iter().any(|effect| matches!(
        effect,
        dexo_tui::Effect::LoadTableData { request, .. }
            if matches!(request.filter, Some(dexo_driver_api::Filter::And(ref parts)) if parts.len() == 2)
    )));
    assert!(model.active_document().related_from.is_some());
    update(&mut model, Action::DataNavBack);
    assert!(model.active_document().related_from.is_none());
    assert_eq!(model.documents.len(), before);
}

#[test]
fn review_enter_emits_apply_changes() {
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
    let effects = update(
        &mut model,
        Action::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::ApplyMutations { .. }))
    );
}

#[test]
fn copy_json_and_next_page_are_wired() {
    let mut model = Model {
        active_session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    // Paging belongs to a table; the scratch document has none to page.
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_table(
            dexo_app::parse_qualified("public.orders"),
            None,
        ));
    model.active_document = 1;
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![DbValue::I64(1)]]);
    model.results.select_cell(0, 0);
    let effects = update(
        &mut model,
        Action::CopyGrid(dexo_app::data::CopyFormat::Json),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::CopyToClipboard { .. }))
    );
    let effects = update(&mut model, Action::NextDataPage);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::LoadTableData { .. }))
    );
}

#[test]
fn page_without_session_does_not_change_offset_or_loading() {
    let mut model = Model::default();
    update(&mut model, Action::NextDataPage);
    assert_eq!(model.data.page_offset, 0);
    assert!(!model.data.loading);
}

/// Filtering a MySQL result re-runs it with MySQL's `?` placeholders. The grid's dialect
/// never followed the connection, so MySQL got Postgres' `$1` and the re-run failed.
#[test]
fn a_mysql_filter_rerun_uses_mysql_placeholders() {
    let mut model = Model::default();
    update(
        &mut model,
        Action::ConnectionChanged {
            name: "shop".into(),
            ready: true,
            environment: "local".into(),
            session: Some(dexo_tui::runtime::SessionId(uuid::Uuid::from_u128(7))),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "mysql".into(),
        },
    );
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id, name from users".into());
    model.results.tabs = vec![tab];
    model.data.filter = Some(dexo_driver_api::Filter::Eq(
        dexo_driver_api::ColumnId("name".into()),
        dexo_driver_api::DbValue::Text("ana".into()),
    ));
    let effects = apply_bars(&mut model);
    let sql = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::StartScript(request) => Some(request.statements[0].clone()),
            _ => None,
        })
        .expect("the filter did not re-run");
    assert!(sql.contains('?') && !sql.contains("$1"), "{sql}");
    assert!(sql.contains("`name`"), "{sql}");
}

/// The WHERE and ORDER BY bars run the statement again with their text, once it reads;
/// a clause that writes is refused and nothing is sent. When the server turns the run
/// down, the rows it had come back.
#[test]
fn the_bars_run_the_result_again_only_with_a_read() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = |code| Action::Key(KeyEvent::new(code, KeyModifiers::NONE));
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id, name from users".into());
    model.results.tabs = vec![tab];
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int8".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![DbValue::I64(7)]]);

    update(
        &mut model,
        Action::FocusClauseBar {
            bar: dexo_tui::screens::data::ClauseBar::Where,
        },
    );
    for ch in "1=1; delete from users".chars() {
        update(&mut model, key(KeyCode::Char(ch)));
    }
    assert!(update(&mut model, key(KeyCode::Enter)).is_empty());

    model.data.bars.where_input.set_text("id > 5");
    let effects = update(&mut model, key(KeyCode::Enter));
    let request = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::StartScript(request) => Some(request.clone()),
            _ => None,
        })
        .expect("ran again");
    assert!(
        request.statements[0].contains("WHERE (id > 5)"),
        "{}",
        request.statements[0]
    );

    update(
        &mut model,
        Action::QueryFailed {
            key: request.key,
            index: 0,
            message: "column \"id\" is ambiguous".into(),
            details: Vec::new(),
            position: None,
        },
    );
    assert_eq!(model.results.rows().len(), 1, "the last good rows are back");
    assert!(model.data.bars.applied.where_sql.is_none());
    assert_eq!(model.data.bars.where_input.as_str(), "id > 5");
}

/// A header click sorts by that column through the ORDER BY bar -- ascending, then
/// descending, then off -- Shift adds a column after the others, and the headers show
/// the order that ran.
#[test]
fn a_header_click_sorts_through_the_order_by_bar() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use dexo_tui::mouse::{HitMap, HitTarget};

    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    model.apply_size(100, 30);
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id, name from users".into());
    model.results.tabs = vec![tab];
    model.results.set_columns(
        ["id", "name"]
            .into_iter()
            .map(|name| dexo_driver_api::ColumnMeta {
                name: name.into(),
                type_name: "text".into(),
                nullable: false,
            })
            .collect(),
    );
    model
        .results
        .append_rows(vec![vec![DbValue::I64(7), DbValue::Text("ana".into())]]);
    let paint = |model: &mut Model| -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = HitMap::default();
        let frame = terminal
            .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
            .unwrap();
        let text: String = frame
            .buffer
            .content()
            .chunks(100)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>() + "\n")
            .collect();
        model.hits = hits;
        text
    };
    let click = |model: &mut Model, column: usize, shift: bool| {
        let (x, y) = model.hits.center(HitTarget::GridHeader(column));
        update(
            model,
            Action::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: if shift {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                },
            }),
        )
    };
    let ran = |effects: &[dexo_tui::Effect]| {
        effects
            .iter()
            .find_map(|effect| match effect {
                dexo_tui::Effect::StartScript(request) => Some(request.statements[0].clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    // The re-run's rows, as the server would send them.
    let answer = |model: &mut Model| {
        model.results.set_columns(
            ["id", "name"]
                .into_iter()
                .map(|name| dexo_driver_api::ColumnMeta {
                    name: name.into(),
                    type_name: "text".into(),
                    nullable: false,
                })
                .collect(),
        );
        model
            .results
            .append_rows(vec![vec![DbValue::I64(7), DbValue::Text("ana".into())]]);
        model.active_operation = None;
    };
    paint(&mut model);
    let effects = click(&mut model, 0, false);
    assert!(
        ran(&effects).contains("ORDER BY id ASC"),
        "{}",
        ran(&effects)
    );
    assert_eq!(model.data.bars.order_input.as_str(), "id ASC");
    answer(&mut model);
    paint(&mut model);
    let effects = click(&mut model, 1, true);
    assert!(
        ran(&effects).contains("ORDER BY id ASC, name ASC"),
        "{}",
        ran(&effects)
    );
    answer(&mut model);
    let screen = paint(&mut model);
    assert!(
        screen.contains("id ▲1") && screen.contains("name ▲2"),
        "{screen}"
    );
    // `s` on the cursor's column cycles it alone: name was ascending, now descending.
    model.results.select_cell(0, 1);
    let effects = update(
        &mut model,
        Action::SortByColumn {
            column: None,
            add: false,
        },
    );
    assert!(
        ran(&effects).contains("ORDER BY name DESC"),
        "{}",
        ran(&effects)
    );
    answer(&mut model);
    model.results.select_cell(0, 1);
    let effects = update(
        &mut model,
        Action::SortByColumn {
            column: None,
            add: false,
        },
    );
    let unsorted = ran(&effects);
    assert!(
        !unsorted.is_empty() && !unsorted.contains("ORDER BY"),
        "{unsorted}"
    );
    answer(&mut model);
    // An ORDER BY no header can show is replaced by a click, and marks nothing.
    model.data.bars.applied.order_by = Some("lower(name)".into());
    assert!(!paint(&mut model).contains('▲'));
    let effects = click(&mut model, 0, false);
    assert!(
        ran(&effects).contains("ORDER BY id ASC"),
        "{}",
        ran(&effects)
    );
}

/// The title says how many rows there are and how sure that is; `t` counts them on a
/// runner of its own, and `t` again cancels the count.
#[test]
fn row_counts_say_whether_they_are_exact_estimated_or_open() {
    use dexo_tui::screens::data::CountState;
    let title = |model: &Model| -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        let mut hits = dexo_tui::mouse::HitMap::default();
        let frame = terminal
            .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
            .unwrap();
        let text: String = frame
            .buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        let start = text.find("Results (").expect("a results title");
        text[start..start + text[start..].find(')').unwrap() + 1].to_string()
    };
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(5))),
        ..Model::default()
    };
    model.apply_size(120, 30);
    let orders = dexo_driver_api::QualifiedName::new(None::<String>, Some("public"), "orders");
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_table(
            orders.clone(),
            None,
        ));
    model.set_active_document(model.documents.len() - 1);
    model.data.target = orders;
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int8".into(),
        nullable: false,
    }]);
    model
        .results
        .append_rows((1..=3).map(|id| vec![DbValue::I64(id)]).collect());
    assert_eq!(title(&model), "Results (3 rows)");
    model.data.has_more = true;
    model.data.page_offset = 100;
    assert_eq!(title(&model), "Results (103+ rows)");
    model.data.estimated_total = Some(4_321_000);
    assert_eq!(title(&model), "Results (~4.3M rows)");

    let effects = update(&mut model, Action::CountRows);
    let (operation, sql) = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::CountRows { operation, sql, .. } => Some((*operation, sql.clone())),
            _ => None,
        })
        .expect("a count started");
    // The table itself, as its page names it.
    assert_eq!(sql, "SELECT COUNT(*) FROM \"public\".\"orders\"");
    assert_eq!(title(&model), "Results (~4.3M rows, counting…)");
    // `t` again cancels it, and its late answer is dropped.
    let effects = update(&mut model, Action::CountRows);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        dexo_tui::Effect::CancelCount { operation: cancelled } if *cancelled == operation
    )));
    update(
        &mut model,
        Action::RowsCounted {
            operation,
            result: Ok(1),
        },
    );
    assert!(model.data.count.is_none());
    let effects = update(&mut model, Action::CountRows);
    let operation = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::CountRows { operation, .. } => Some(*operation),
            _ => None,
        })
        .unwrap();
    update(
        &mut model,
        Action::RowsCounted {
            operation,
            result: Ok(4_321_987),
        },
    );
    assert_eq!(
        model.data.count.as_ref().map(|count| count.state),
        Some(CountState::Exact(4_321_987))
    );
    assert_eq!(title(&model), "Results (4,321,987 rows)");
    // A WHERE that ran since makes it another count: the exact number goes.
    model.data.bars.applied.where_sql = Some("id > 2".into());
    model.data.estimated_total = None;
    assert_eq!(title(&model), "Results (103+ rows)");

    // A statement's rows that stopped at the limit say so.
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.truncated = true;
    let query = model.documents.len();
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_unique(
            "q.sql", None, None,
        ));
    model.set_active_document(query);
    model.results.tabs = vec![tab];
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int8".into(),
        nullable: false,
    }]);
    model
        .results
        .append_rows((1..=3).map(|id| vec![DbValue::I64(id)]).collect());
    assert_eq!(title(&model), "Results (3+ rows, limit reached)");
}

/// `f` on a row lists the keys from and to its table; the chosen one opens the other
/// table filtered to the row's key -- every column of a composite one -- and `b` comes
/// back to the row it left. A NULL key opens nothing.
#[test]
fn related_rows_open_both_ways_and_back_returns_to_the_row() {
    use dexo_driver_api::{ColumnId, Filter, ForeignKeyRef, QualifiedName};
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(5))),
        session_generation: 2,
        ..Model::default()
    };
    let named = |name: &str| QualifiedName::new(Some("shop"), Some("public"), name);
    model
        .documents
        .push(dexo_tui::model::EditorDocument::new_table(
            named("orders"),
            None,
        ));
    let orders_doc = model.documents.len() - 1;
    model.set_active_document(orders_doc);
    model.data.target = named("orders");
    model.results.set_columns(
        ["id", "region", "customer_id"]
            .into_iter()
            .map(|name| dexo_driver_api::ColumnMeta {
                name: name.into(),
                type_name: "text".into(),
                nullable: true,
            })
            .collect(),
    );
    model.results.append_rows(vec![
        vec![DbValue::I64(1), DbValue::Text("eu".into()), DbValue::Null],
        vec![DbValue::I64(2), DbValue::Text("us".into()), DbValue::I64(9)],
    ]);
    model.results.select_cell(1, 0);
    let keys = vec![
        ForeignKeyRef {
            name: "orders_customer".into(),
            from: named("orders"),
            from_columns: vec!["customer_id".into()],
            to: named("customers"),
            to_columns: vec!["id".into()],
        },
        ForeignKeyRef {
            name: "lines_order".into(),
            from: named("lines"),
            from_columns: vec!["order_id".into(), "order_region".into()],
            to: named("orders"),
            to_columns: vec!["id".into(), "region".into()],
        },
    ];
    let pick = |model: &mut Model, keys: &[ForeignKeyRef]| {
        let effects = update(model, Action::OpenRelatedPicker);
        let table = effects
            .iter()
            .find_map(|effect| match effect {
                dexo_tui::Effect::LoadForeignKeys { table, .. } => Some(table.clone()),
                _ => None,
            })
            .expect("the keys were asked for");
        update(
            model,
            Action::ForeignKeysLoaded {
                generation: 2,
                table,
                result: Ok(keys.to_vec()),
            },
        );
    };
    let filter_of = |effects: &[dexo_tui::Effect]| {
        effects.iter().find_map(|effect| match effect {
            dexo_tui::Effect::LoadTableData { request, .. } => {
                Some((request.object.object().to_string(), request.filter.clone()))
            }
            _ => None,
        })
    };
    let key = |code| {
        Action::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ))
    };

    pick(&mut model, &keys);
    let labels: Vec<String> = model
        .data
        .related_picker
        .as_ref()
        .and_then(|picker| picker.links.as_ref())
        .unwrap()
        .iter()
        .map(|link| link.label.clone())
        .collect();
    assert_eq!(
        labels,
        [
            "→ customers (customer_id)",
            "← lines (order_id, order_region)"
        ]
    );
    let effects = update(&mut model, key(crossterm::event::KeyCode::Enter));
    assert_eq!(
        filter_of(&effects),
        Some((
            "customers".into(),
            Some(Filter::Eq(ColumnId("id".into()), DbValue::I64(9)))
        ))
    );
    assert!(model.data.related_picker.is_none());
    // The filter the key brought is said, in the title and in the console's log.
    let screen = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(screen.contains("where id = 9"), "{screen}");
    assert!(
        model
            .active_document()
            .console_log
            .iter()
            .any(|line| line.contains("SELECT * FROM shop.public.customers WHERE id = 9 LIMIT")),
        "{:?}",
        model.active_document().console_log
    );

    update(&mut model, Action::DataNavBack);
    assert_eq!(model.active_document, orders_doc);
    assert_eq!(
        model.results.selection(),
        Some((1, 0)),
        "back is the row it left"
    );

    pick(&mut model, &keys);
    update(&mut model, key(crossterm::event::KeyCode::Down));
    let effects = update(&mut model, key(crossterm::event::KeyCode::Enter));
    assert_eq!(
        filter_of(&effects),
        Some((
            "lines".into(),
            Some(Filter::And(vec![
                Filter::Eq(ColumnId("order_id".into()), DbValue::I64(2)),
                Filter::Eq(ColumnId("order_region".into()), DbValue::Text("us".into())),
            ]))
        ))
    );
    update(&mut model, Action::DataNavBack);

    // The first row's customer is NULL: nothing to open.
    model.results.select_cell(0, 0);
    pick(&mut model, &keys);
    let effects = update(&mut model, key(crossterm::event::KeyCode::Enter));
    assert_eq!(filter_of(&effects), None);
    assert_eq!(model.active_document, orders_doc);
}

/// From the keyboard, Right moves the current column, and `S` adds that column to the
/// sort -- not the first column every time.
#[test]
fn the_keyboard_sorts_the_column_it_is_on() {
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    model.apply_size(100, 30);
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id, customer_id from orders".into());
    model.results.tabs = vec![tab];
    model.results.set_columns(
        ["id", "customer_id"]
            .into_iter()
            .map(|name| dexo_driver_api::ColumnMeta {
                name: name.into(),
                type_name: "int".into(),
                nullable: false,
            })
            .collect(),
    );
    model
        .results
        .append_rows(vec![vec![DbValue::I64(1), DbValue::I64(9)]]);
    model.results.select_row(0);
    update(&mut model, Action::ResultsRight);
    assert_eq!(model.results.selection().map(|(_, col)| col), Some(1));
    update(
        &mut model,
        Action::SortByColumn {
            column: None,
            add: true,
        },
    );
    assert!(
        model.data.bars.order_input.as_str().contains("customer_id"),
        "{}",
        model.data.bars.order_input.as_str()
    );
}

/// A sort is refused while a statement of the session runs, or while row edits wait to
/// be applied; the bar's text is left as it was, and nothing runs.
#[test]
fn a_sort_waits_for_the_running_statement_and_the_pending_edits() {
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id from orders".into());
    model.results.tabs = vec![tab];
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![DbValue::I64(1)]]);
    let sort = |model: &mut Model| {
        update(
            model,
            Action::SortByColumn {
                column: Some(0),
                add: false,
            },
        )
    };
    model.active_operation = Some(dexo_tui::runtime::OperationId::new());
    assert!(sort(&mut model).is_empty());
    assert_eq!(model.data.bars.order_input.as_str(), "");
    model.active_operation = None;
    model
        .data
        .row_changes
        .insert(0, dexo_app::data::RowEditState::Deleted);
    assert!(sort(&mut model).is_empty());
    assert!(model.data.bars.applied.order_by.is_none());
    model.data.row_changes.clear();
    assert!(!sort(&mut model).is_empty());
}

fn result_with_bars() -> Model {
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        ..Model::default()
    };
    model.apply_size(100, 30);
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id from orders".into());
    model.results.tabs = vec![tab];
    model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
        name: "id".into(),
        type_name: "int".into(),
        nullable: false,
    }]);
    model.results.append_rows(vec![vec![DbValue::I64(1)]]);
    model
}

/// The terminal cursor sits right after the ORDER BY text; leaving the pane takes the
/// bar's focus with it; `w` from the log comes back to the grid's bar; a paste on the
/// grid runs nothing, and one into a bar keeps to one line.
#[test]
fn the_bars_keep_their_cursor_focus_and_paste_in_place() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use dexo_tui::screens::data::ClauseBar;
    let mut model = result_with_bars();
    update(
        &mut model,
        Action::FocusClauseBar {
            bar: ClauseBar::Order,
        },
    );
    model.data.bars.order_input.set_text("id");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut hits = dexo_tui::mouse::HitMap::default();
    let frame = terminal
        .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
        .unwrap();
    let rows: Vec<String> = frame
        .buffer
        .content()
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let (y, row) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains("ORDER BY id"))
        .expect("the bars are drawn");
    let byte = row.find("ORDER BY id").unwrap();
    let x = row[..byte].chars().count() + "ORDER BY id".len();
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!((cursor.x as usize, cursor.y as usize), (x, y));

    model.focus = dexo_tui::Focus::Explorer;
    update(
        &mut model,
        Action::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
    );
    assert_eq!(model.data.bars.order_input.as_str(), "id");
    assert_eq!(model.data.bars.focus, None);

    model.focus = dexo_tui::Focus::Results;
    model.results.view = dexo_tui::model::ResultsView::Messages;
    update(
        &mut model,
        Action::FocusClauseBar {
            bar: ClauseBar::Where,
        },
    );
    assert_eq!(model.results.view, dexo_tui::model::ResultsView::Grid);
    update(&mut model, Action::Paste("a = 1 and\nb = 2".into()));
    assert_eq!(model.data.bars.where_input.as_str(), "a = 1 and b = 2");

    model.data.bars.focus = None;
    let effects = update(&mut model, Action::Paste("st".into()));
    assert!(effects.is_empty());
    assert!(model.data.bars.applied.order_by.is_none());
}

/// Follows `key` from the row in `related_row`, as Enter in the related-rows picker does.
fn follow(model: &mut Model, key: dexo_app::data::ForeignKey) -> Vec<dexo_tui::Effect> {
    model.data.related_picker = Some(dexo_tui::screens::data::RelatedPicker {
        table: model.data.target.clone(),
        links: Some(vec![dexo_tui::screens::data::RelatedLink {
            label: "related".into(),
            key,
        }]),
        selected: 0,
    });
    update(
        model,
        Action::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )),
    )
}

/// Alt+click and a right click on a header add the column to the sort: most terminals
/// keep Shift+click for their own selection.
#[test]
fn alt_click_and_right_click_add_a_column_to_the_sort() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use dexo_tui::mouse::{HitMap, HitTarget};
    for (button, modifiers) in [
        (MouseButton::Left, KeyModifiers::ALT),
        (MouseButton::Right, KeyModifiers::NONE),
    ] {
        let mut model = Model {
            focus: dexo_tui::Focus::Results,
            ..Model::default()
        };
        model.apply_size(100, 30);
        let mut tab = ResultTab::new(result_key(0), "r0");
        tab.source_sql = Some("select id, name from users".into());
        model.results.tabs = vec![tab];
        model.results.set_columns(
            ["id", "name"]
                .into_iter()
                .map(|name| dexo_driver_api::ColumnMeta {
                    name: name.into(),
                    type_name: "text".into(),
                    nullable: false,
                })
                .collect(),
        );
        model
            .results
            .append_rows(vec![vec![DbValue::I64(7), DbValue::Text("ana".into())]]);
        model.data.bars.applied.order_by = Some("id ASC".into());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        let mut hits = HitMap::default();
        terminal
            .draw(|frame| dexo_tui::render::render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let (x, y) = model.hits.center(HitTarget::GridHeader(1));
        update(
            &mut model,
            Action::Mouse(MouseEvent {
                kind: MouseEventKind::Down(button),
                column: x,
                row: y,
                modifiers,
            }),
        );
        assert_eq!(
            model.data.bars.order_input.as_str(),
            "id ASC, name ASC",
            "{button:?} {modifiers:?}"
        );
    }
}

/// Runs the grid again with the bars, as Enter in the WHERE bar does.
fn apply_bars(model: &mut Model) -> Vec<dexo_tui::Effect> {
    update(
        model,
        Action::FocusClauseBar {
            bar: dexo_tui::screens::data::ClauseBar::Where,
        },
    );
    update(
        model,
        Action::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )),
    )
}

/// A result run again with the bars is one page: a full page says more may follow, and
/// n runs the statement again at the next offset.
#[test]
fn a_result_run_again_with_the_bars_pages() {
    let mut model = Model {
        focus: dexo_tui::Focus::Results,
        active_session: Some(dexo_tui::runtime::SessionId(Uuid::from_u128(1))),
        session_generation: 1,
        ..Model::default()
    };
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select n from generate_series(1, 500) n".into());
    model.results.tabs = vec![tab];
    let effects = apply_bars(&mut model);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, dexo_tui::Effect::StartScript(_)))
    );
    let page = |model: &mut Model, rows: i64| {
        model.results.set_columns(vec![dexo_driver_api::ColumnMeta {
            name: "n".into(),
            type_name: "int".into(),
            nullable: false,
        }]);
        model
            .results
            .append_rows((0..rows).map(|n| vec![DbValue::I64(n)]).collect());
        model.active_operation = None;
    };
    let limit = i64::from(model.data.page_limit);
    page(&mut model, limit);
    let title = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(title.contains("Results (100+ rows)"), "{title}");
    let effects = update(&mut model, Action::NextDataPage);
    let sql = effects
        .iter()
        .find_map(|effect| match effect {
            dexo_tui::Effect::StartScript(request) => Some(request.statements[0].clone()),
            _ => None,
        })
        .expect("the next page runs");
    assert!(sql.contains("OFFSET 100"), "{sql}");
    page(&mut model, 7);
    let title = dexo_tui::render::render_to_string(&model, 120, 30);
    assert!(title.contains("Results (107 rows)"), "{title}");
    assert!(update(&mut model, Action::NextDataPage).is_empty());
}

/// A re-run around the bars' text is sent to run where it cannot write.
#[test]
fn a_re_run_with_the_bars_only_reads() {
    let mut model = Model::default();
    let mut tab = ResultTab::new(result_key(0), "r0");
    tab.source_sql = Some("select id from users".into());
    model.results.tabs = vec![tab];
    let effects = apply_bars(&mut model);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        dexo_tui::Effect::StartScript(request) if request.read_only
    )));
}
