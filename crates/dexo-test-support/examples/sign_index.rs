//! Signs a file with a fresh test key, for trying a local plugin index by hand:
//! `cargo run -p dexo-test-support --example sign_index -- index.json` writes
//! `index.json.minisig` and prints the public key for `DEXO_PLUGIN_TRUSTED_KEY`.
fn main() {
    let path = std::env::args().nth(1).expect("the file to sign");
    let key = dexo_test_support::plugins::TestKey::new();
    let data = std::fs::read(&path).expect("the file");
    std::fs::write(format!("{path}.minisig"), key.sign(&data)).expect("the signature");
    println!("{}", key.public());
}
