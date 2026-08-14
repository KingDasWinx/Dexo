use crate::screens::schema_editor::DdlPreviewState;

pub fn preview_lines(preview: &DdlPreviewState) -> Vec<String> {
    preview.lines()
}
