//! The plugin's manifest and the crate agree: the release checks the tag against both.
#[test]
fn the_manifest_names_this_crate_and_its_version() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/duckdb/dexo-plugin.toml");
    let manifest =
        dexo_app::plugin::manifest::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(manifest.id, "duckdb");
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    let driver = manifest.driver.unwrap();
    assert_eq!(driver.executable, "bin/dexo-driver-duckdb");
    assert_eq!(driver.drivers, ["duckdb"]);
}
