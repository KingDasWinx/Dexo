use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

pub fn picker() -> Picker {
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}

pub fn protocol(bytes: &[u8]) -> Option<StatefulProtocol> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(picker().new_resize_protocol(img))
}

pub fn metadata_line(width: u32, height: u32, mime: &str) -> String {
    format!("{mime} {width}x{height}")
}

#[cfg(test)]
mod tests {
    use super::metadata_line;

    #[test]
    fn metadata_line_includes_mime_and_size() {
        assert_eq!(metadata_line(2, 3, "image/png"), "image/png 2x3");
    }
}
