use assert_cmd::Command;
use predicates::prelude::*;

fn gutcheck() -> Command {
    Command::cargo_bin("gutcheck").expect("binary should build")
}

#[test]
fn version_prints_name_and_version() {
    gutcheck()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("gutcheck {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn no_arguments_prints_help() {
    gutcheck()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage: gutcheck"));
}
