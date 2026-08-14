use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_uses_stdout() {
    Command::cargo_bin("dexo")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("dexo 0.1.0"));
}

#[test]
fn doctor_is_non_interactive() {
    Command::cargo_bin("dexo")
        .unwrap()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""status":"ok""#));
}
