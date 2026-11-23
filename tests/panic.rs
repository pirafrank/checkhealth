//! Panics must map to exit code 5. Needs the `test-hooks` feature (`cargo test --all-features`).

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn panic_exits_with_internal_error() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.yml");
    std::fs::write(&config, "commands: [sh]\n").unwrap();
    Command::cargo_bin("checkhealth")
        .unwrap()
        .arg("-c")
        .arg(&config)
        .env("CHECKHEALTH_TEST_PANIC", "1")
        .assert()
        .code(5)
        .stdout("")
        .stderr(
            predicate::str::starts_with("error: internal error: induced")
                .and(predicate::str::contains("/issues")),
        );
}
