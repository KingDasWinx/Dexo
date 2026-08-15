use dexo_app::{ExecutionTarget, statements_for};

use crate::model::Model;

pub fn planned_statements(model: &Model) -> Vec<String> {
    let doc = model.active_document();
    statements_for(
        &doc.text(),
        model.execution_target,
        doc.cursor(),
        doc.selection(),
    )
}

pub fn execute_current_statement(model: &mut Model) {
    model.execution_target = ExecutionTarget::CurrentStatement;
}

pub fn execute_selection(model: &mut Model) {
    model.execution_target = ExecutionTarget::Selection;
}

pub fn execute_document(model: &mut Model) {
    model.execution_target = ExecutionTarget::Document;
}

#[cfg(test)]
mod tests {
    use crate::action::Action;
    use crate::model::Model;
    use crate::update;

    #[test]
    fn script_creates_result_tabs_in_order() {
        let mut model = Model::default();
        model.set_sql("select 1; select 2; select 3;");
        update(&mut model, Action::ExecuteQuery);
        assert_eq!(model.result_tabs.len(), 3);
    }
}
