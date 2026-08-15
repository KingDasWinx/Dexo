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
