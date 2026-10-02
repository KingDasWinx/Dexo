use crate::screens::schema_editor::DdlPreviewState;

pub fn preview_lines(preview: &DdlPreviewState, rows: usize) -> Vec<String> {
    preview.lines(rows)
}
