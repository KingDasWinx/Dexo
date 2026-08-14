#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = dexo_sql::split_statements(&String::from_utf8_lossy(data));
});
