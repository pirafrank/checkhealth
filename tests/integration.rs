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

/// Directory holding symlinks to a few real tools, so `PATH` is small and predictable.
fn tool_dir(tools: &[&str]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for tool in tools {
        let source = which::which(tool).expect("tool available on this machine");
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, dir.path().join(tool)).unwrap();
    }
    dir
}

#[test]
fn found_commands_exit_0_and_show_the_path() {
    let tools = tool_dir(&["sh", "ls"]);
    let (_dir, path) = config("commands: [sh, ls]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .arg("--no-version")
        .env("PATH", tools.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[✓] sh  "))
        .stdout(predicate::str::contains("2/2 found"));
}

#[test]
fn missing_command_exits_1() {
    let tools = tool_dir(&["sh"]);
    let (_dir, path) = config("commands: [sh, nonexistent-tool-1]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .env("PATH", tools.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("[✗] nonexistent-tool-1\n"))
        .stdout(predicate::str::contains("1/2 found"))
        .stderr("");
}

#[test]
fn output_follows_config_order_and_deduplicates() {
    let tools = tool_dir(&["sh", "ls", "cat"]);
    let (_dir, path) = config("commands: [ls, nonexistent-tool-1, sh, ls, cat]\n");
    let output = checkhealth()
        .arg("-c")
        .arg(&path)
        .arg("--no-version")
        .env("PATH", tools.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let names: Vec<_> = stdout
        .lines()
        .take_while(|l| !l.is_empty())
        .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
        .collect();
    assert_eq!(names, ["ls", "nonexistent-tool-1", "sh", "cat"]);
}

#[test]
fn invalid_shell_is_exit_4() {
    let (_dir, path) = config("commands: [sh]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .args(["--shell", "fish"])
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicate::str::contains("unsupported shell `fish`"));
}

#[test]
fn nonexistent_shell_path_is_exit_4() {
    let (_dir, path) = config("commands: [sh]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .args(["--shell", "/nonexistent/bash"])
        .assert()
        .code(4);
}

#[test]
fn colour_is_off_when_piped_and_with_no_color() {
    let tools = tool_dir(&["sh"]);
    let (_dir, path) = config("commands: [sh]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .env("PATH", tools.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[").not());
    checkhealth()
        .arg("-c")
        .arg(&path)
        .args(["--color", "never"])
        .env("PATH", tools.path())
        .env("CLICOLOR_FORCE", "1")
        .assert()
        .stdout(predicate::str::contains("\x1b[").not());
}

#[test]
fn colour_can_be_forced() {
    let tools = tool_dir(&["sh"]);
    let (_dir, path) = config("commands: [sh]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .args(["--color", "always"])
        .env("PATH", tools.path())
        .assert()
        .stdout(predicate::str::contains("\x1b[32m[✓]"));
    checkhealth()
        .arg("-c")
        .arg(&path)
        .env("PATH", tools.path())
        .env("CLICOLOR_FORCE", "1")
        .assert()
        .stdout(predicate::str::contains("\x1b[32m[✓]"));
}

#[test]
fn no_color_env_beats_forcing_by_environment() {
    let tools = tool_dir(&["sh"]);
    let (_dir, path) = config("commands: [sh]\n");
    checkhealth()
        .arg("-c")
        .arg(&path)
        .env("PATH", tools.path())
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .assert()
        .stdout(predicate::str::contains("\x1b[").not());
}
