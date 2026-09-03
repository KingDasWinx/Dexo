use dexo_app::{ExecutionTarget, statements_for};

use crate::model::Model;

pub fn planned_statements(model: &Model) -> Vec<String> {
    let doc = model.active_document();
    let sql = doc.text();
    let cursor = char_to_byte_index(&sql, doc.cursor());
    let selection = doc
        .selection()
        .map(|range| char_to_byte_index(&sql, range.start)..char_to_byte_index(&sql, range.end));
    statements_for(&sql, model.execution_target, cursor, selection)
}

fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(text.len())
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{execute_current_statement, execute_selection, planned_statements};
    use crate::action::Action;
    use crate::model::Model;
    use crate::update;

    #[test]
    fn script_creates_result_tabs_in_order() {
        let mut model = Model::default();
        model.set_sql("select 1; select 2; select 3;");
        update(&mut model, Action::ExecuteDocument);
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

    #[test]
    fn ctrl_enter_key_executes_the_current_statement() {
        let mut model = Model::default();
        model.set_sql("select 1; select 2;");

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        );

        assert_eq!(
            model.execution_target,
            dexo_app::ExecutionTarget::CurrentStatement
        );
        assert_eq!(model.results.tabs.len(), 1);
    }

    #[test]
    fn ctrl_enter_key_executes_the_editor_selection() {
        let mut model = Model::default();
        model.set_sql("select 1; select 2;");
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );

        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        );

        assert_eq!(model.execution_target, dexo_app::ExecutionTarget::Selection);
        assert_eq!(model.results.tabs.len(), 2);
    }

    #[test]
    fn current_statement_cursor_is_unicode_safe() {
        let sql = "select '😀😀😀😀😀'; select 2;";
        let mut model = Model::default();
        model.set_sql(sql);
        let second_start = sql[..sql.find("select 2").unwrap()].chars().count();
        let _ = model.active_document_mut().sql.set_cursor(second_start + 3);
        execute_current_statement(&mut model);

        assert_eq!(planned_statements(&model), ["select 2"]);
    }

    #[test]
    fn selected_statement_range_is_unicode_safe() {
        let sql = "select '😀😀😀😀😀'; select 2;";
        let mut model = Model::default();
        model.set_sql(sql);
        let second_start = sql[..sql.find("select 2").unwrap()].chars().count();
        let second_end = second_start + "select 2".chars().count();
        let _ = model.active_document_mut().sql.set_cursor(second_start);
        crate::screens::editor::extend_selection_to(&mut model, second_end);
        execute_selection(&mut model);

        assert_eq!(planned_statements(&model), ["select 2"]);
    }
}
