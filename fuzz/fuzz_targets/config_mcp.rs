#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = dexo_app::mcp::Selector::parse(&text);
    let _ = dexo_app::diagnostic_service::redact_text(&text);
    let _ = dexo_storage::Preferences::from_toml(&text);
});
