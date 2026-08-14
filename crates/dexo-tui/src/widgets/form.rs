use crate::screens::schema_editor::{FormField, SchemaEditor};

pub fn render_lines(editor: &SchemaEditor) -> Vec<String> {
    editor.lines()
}

pub fn focused_field(editor: &SchemaEditor) -> Option<&FormField> {
    editor.fields.get(editor.focus)
}

#[cfg(test)]
mod tests {
    use super::{focused_field, render_lines};
    use crate::screens::schema_editor::SchemaEditor;

    #[test]
    fn focus_stays_on_selected_field() {
        let mut editor = SchemaEditor::table_form("public.t");
        editor.focus = 1;
        assert_eq!(focused_field(&editor).unwrap().label, "columns");
        assert!(
            render_lines(&editor)
                .iter()
                .any(|line| line.starts_with("> columns:"))
        );
    }
}
