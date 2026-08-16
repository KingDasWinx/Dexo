use dexo_app::data::{ValueView, inspect_value};
use dexo_driver_api::DbValue;

use crate::widgets::image_viewer;

pub fn view(value: &DbValue) -> ValueView {
    let loaded = match value {
        DbValue::Bytes(bytes) | DbValue::Native { bytes, .. } => bytes.len() as u64,
        DbValue::Text(text) | DbValue::Json(text) => text.len() as u64,
        _ => 0,
    };
    let view = inspect_value(value, loaded, loaded);
    if let ValueView::Image {
        mime,
        width: Some(width),
        height: Some(height),
    } = &view
    {
        let _ = image_viewer::metadata_line(*width, *height, mime);
    }
    view
}
