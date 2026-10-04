use crate::screens::schema_editor::DdlPreviewState;

pub fn preview_lines(preview: &DdlPreviewState, rows: usize, width: usize) -> (Vec<String>, usize) {
    preview.lines(rows, width)
}
