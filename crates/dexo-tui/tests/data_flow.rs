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
        ResultTab {
            key: result_key(0),
            title: "r0".into(),
            grid: dexo_tui::GridModel::default(),
            status: OperationStatus::Running,
            rows_affected: None,
            notices: Vec::new(),
        },
        ResultTab {
            key: result_key(1),
            title: "r1".into(),
            grid: dexo_tui::GridModel::default(),
            status: OperationStatus::Running,
            rows_affected: None,
            notices: Vec::new(),
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
