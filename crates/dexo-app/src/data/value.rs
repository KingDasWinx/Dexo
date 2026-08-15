use std::path::Path;

use dexo_driver_api::DbValue;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchToken(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum ValueView {
    Null,
    Text(String),
    JsonPretty(String),
    Xml(String),
    Array(String),
    Hex(String),
    Image {
        mime: &'static str,
        width: Option<u32>,
        height: Option<u32>,
    },
    Truncated {
        loaded: u64,
        total: u64,
    },
    Unloaded {
        token: FetchToken,
        total: u64,
    },
}

pub const INLINE_BYTES: u64 = 64 * 1024;
pub const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

pub fn inspect_value(value: &DbValue, loaded: u64, total: u64) -> ValueView {
    if loaded < total {
        return ValueView::Truncated { loaded, total };
    }
    match value {
        DbValue::Null => ValueView::Null,
        DbValue::Json(text) => ValueView::JsonPretty(pretty_json(text)),
        DbValue::Text(text) if text.trim_start().starts_with('<') => ValueView::Xml(text.clone()),
        DbValue::Text(text) if text.starts_with('{') || text.starts_with('[') => {
            ValueView::JsonPretty(pretty_json(text))
        }
        DbValue::Text(text) => ValueView::Text(text.clone()),
        DbValue::Bytes(bytes) => inspect_bytes(bytes, total),
        DbValue::Native { text, bytes, .. } => {
            if bytes.len() as u64 > INLINE_BYTES {
                ValueView::Truncated {
                    loaded: INLINE_BYTES,
                    total: bytes.len() as u64,
                }
            } else if text.starts_with('{') || text.starts_with('[') {
                ValueView::JsonPretty(pretty_json(text))
            } else {
                ValueView::Text(text.clone())
            }
        }
        other => ValueView::Text(format!("{other:?}")),
    }
}

fn inspect_bytes(bytes: &[u8], total: u64) -> ValueView {
    if total > INLINE_BYTES && bytes.len() as u64 <= INLINE_BYTES {
        return ValueView::Truncated {
            loaded: bytes.len() as u64,
            total,
        };
    }
    if let Some(mime) = image_mime(bytes) {
        let (width, height) = image::load_from_memory(bytes)
            .ok()
            .map(|img| (img.width(), img.height()))
            .unzip();
        return ValueView::Image {
            mime,
            width,
            height,
        };
    }
    if bytes.first() == Some(&b'[') {
        return ValueView::Array(String::from_utf8_lossy(bytes).into_owned());
    }
    ValueView::Hex(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    // ponytail: magic bytes only, never file extension. Ceiling: PNG/JPEG/GIF/WEBP; add more signatures if needed.
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn pretty_json(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| text.to_string())
}

pub fn fetch_on_demand(token: FetchToken, total: u64) -> ValueView {
    if total > MAX_DOWNLOAD_BYTES {
        ValueView::Truncated { loaded: 0, total }
    } else {
        ValueView::Unloaded { token, total }
    }
}

pub fn save_bytes_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::{
        FetchToken, MAX_DOWNLOAD_BYTES, ValueView, fetch_on_demand, inspect_value,
        save_bytes_atomic,
    };
    use dexo_driver_api::DbValue;

    #[test]
    fn large_and_native_views() {
        assert!(matches!(
            inspect_value(&DbValue::Json("{\"a\":1}".into()), 8, 8),
            ValueView::JsonPretty(text) if text.contains('\n')
        ));
        assert!(matches!(
            inspect_value(&DbValue::Text("<root/>".into()), 7, 7),
            ValueView::Xml(_)
        ));
        assert!(matches!(
            inspect_value(&DbValue::Bytes(b"[1,2]".to_vec()), 5, 5),
            ValueView::Array(_)
        ));
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert!(matches!(
            inspect_value(
                &DbValue::Bytes(png.to_vec()),
                png.len() as u64,
                png.len() as u64
            ),
            ValueView::Image {
                mime: "image/png",
                ..
            }
        ));
        let truncated = inspect_value(&DbValue::Bytes(vec![0; 16]), 16, 100 * 1024 * 1024);
        assert_eq!(
            truncated,
            ValueView::Truncated {
                loaded: 16,
                total: 100 * 1024 * 1024
            }
        );
        assert!(matches!(
            fetch_on_demand(FetchToken("blob".into()), MAX_DOWNLOAD_BYTES + 1),
            ValueView::Truncated { loaded: 0, total } if total == MAX_DOWNLOAD_BYTES + 1
        ));
        let dir = std::env::temp_dir().join(format!("dexo-value-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob.bin");
        save_bytes_atomic(&path, b"ok").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ok");
        let _ = std::fs::remove_dir_all(dir);
    }
}
