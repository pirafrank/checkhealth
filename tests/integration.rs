//! Integration tests: run the compiled binary and check its output and exit codes.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Returns the binary under test.
fn checkhealth() -> Command {
    Command::cargo_bin("checkhealth").expect("binary builds")
}

/// Writes `content` to `config.yml` in a fresh temp dir; keep the dir alive while in use.
fn config(content: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.yml");
    std::fs::write(&path, content).unwrap();
    (dir, path)
}

#[test]
fn version_is_the_long_form() {
    checkhealth()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("checkhealth"))
        .stdout(predicate::str::contains("Commit"));
}

#[test]
fn help_documents_exit_codes_and_options() {
    checkhealth().arg("--help").assert().success().stdout(
        predicate::str::contains("--config")
            .and(predicate::str::contains("--no-version"))
            .and(predicate::str::contains("--timeout"))
            .and(predicate::str::contains("--color"))
            .and(predicate::str::contains("Exit codes")),
    );
}

#[test]
fn completions_do_not_need_a_config() {
    checkhealth()
        .args(["completions", "--shell", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkhealth"));
}

#[test]
fn missing_config_flag_is_a_usage_error() {
    checkhealth().assert().code(2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    checkhealth().arg("--bogus").assert().code(2);
}

#[test]
fn bad_timeout_is_a_usage_error() {
    checkhealth()
        .args(["-c", "x", "--timeout", "0"])
        .assert()
        .code(2);
}

#[test]
fn missing_config_file_is_exit_3() {
    checkhealth()
        .args(["-c", "/nonexistent/config.yml"])
        .assert()
        .code(3)
        .stderr(predicate::str::starts_with(
            "error: failed to read config /nonexistent",
        ));
}

#[test]
fn bad_yaml_is_exit_3() {
    let (_dir, path) = config("commands: [unclosed");
    checkhealth().arg("-c").arg(&path).assert().code(3);
}

#[test]
fn unknown_key_is_exit_3() {
    let (_dir, path) = config("command: [cargo]");
    checkhealth().arg("-c").arg(&path).assert().code(3);
}

#[test]
fn invalid_name_is_exit_3() {
    let (_dir, path) = config("commands: [\"a; rm -rf x\"]");
    checkhealth().arg("-c").arg(&path).assert().code(3);
}
