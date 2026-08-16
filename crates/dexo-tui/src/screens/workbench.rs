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
        assert_eq!(model.results.tabs.len(), 3);
    }

    #[test]
    fn execute_statement_uses_current_statement_target() {
        let mut model = Model::default();
        model.set_sql("select 1; select 2;");
        let _ = model.active_document_mut().sql.set_cursor(0);
        crate::screens::workbench::execute_current_statement(&mut model);
        assert_eq!(
            model.execution_target,
            dexo_app::ExecutionTarget::CurrentStatement
        );
        update(&mut model, Action::ExecuteStatement);
        assert_eq!(model.results.tabs.len(), 1);
    }
}
