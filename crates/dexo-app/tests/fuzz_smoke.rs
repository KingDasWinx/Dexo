use dexo_app::diagnostic_service::redact_text;
use dexo_app::mcp::selector::Selector;
use dexo_app::transfer::{FormatOptions, TransferFormat, decode_document, encode_document};
use dexo_driver_api::DbValue;

#[test]
fn fuzz_smoke_redaction_and_selectors() {
    for sample in [
        "",
        "password=SUPER_SECRET_SENTINEL",
        "db.public.*",
        "***",
        "postgres://u:SUPER_SECRET_SENTINEL@localhost/db",
    ] {
        let _ = redact_text(sample);
        let _ = Selector::parse(sample);
    }
}

#[test]
fn fuzz_smoke_transfer_codec() {
    let options = FormatOptions::default();
    let encoded = encode_document(
        TransferFormat::Csv,
        &options,
        &["id".into()],
        &[vec![DbValue::I64(1)], vec![DbValue::Null]],
    )
    .unwrap();
    let _ = decode_document(TransferFormat::Csv, &options, &encoded);
    for bytes in [b"" as &[u8], b"{", b"\xff\x00", b"id\n1"] {
        let _ = decode_document(TransferFormat::Csv, &options, bytes);
        let _ = decode_document(TransferFormat::Json, &options, bytes);
    }
}
