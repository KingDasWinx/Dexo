#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let options = dexo_app::transfer::FormatOptions::default();
    let _ = dexo_app::transfer::decode_document(
        dexo_app::transfer::TransferFormat::Csv,
        &options,
        data,
    );
    let _ = dexo_app::transfer::decode_document(
        dexo_app::transfer::TransferFormat::Json,
        &options,
        data,
    );
});
