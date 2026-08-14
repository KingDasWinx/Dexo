use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use dexo_app::transfer::codec::{FormatOptions, TransferFormat};
use dexo_app::transfer::export::export_rows;
use dexo_driver_api::DbValue;

fn main() {
    let columns = vec!["id".into(), "name".into()];
    let rows = (0..10_000).map(|i| vec![DbValue::I64(i), DbValue::Text("n".into())]);
    let dest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/export.csv");
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let started = Instant::now();
    let progress = export_rows(
        &dest,
        TransferFormat::Csv,
        &FormatOptions::default(),
        &columns,
        rows,
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let payload = format!(
        "{{\"metric\":\"export_stream\",\"ms\":{ms},\"rows\":{},\"bytes\":{},\"bounded\":true}}",
        progress.rows, progress.bytes
    );
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/results/export.json");
    fs::write(&path, payload.as_bytes()).unwrap();
    println!("{payload}");
}
