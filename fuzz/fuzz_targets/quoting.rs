#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = dexo_driver_postgres::PgDialect::quote_ident(&text);
    let _ = dexo_driver_mysql::MysqlDialect::quote_ident(&text);
});
