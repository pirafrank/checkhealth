//! Best-effort version probing: `<tool> --version`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use log::debug;

use crate::process::{self, Limits, Outcome};

/// Default time a tool gets to print its version.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
/// Output read from a probe; enough for any sane `--version`.
const MAX_OUTPUT: usize = 4096;
/// Longest version string shown; the rest is cut.
const MAX_LINE_CHARS: usize = 120;

/// Runs `<program> --version` and returns the first non-empty line of stdout, falling back to
/// stderr. Returns `None` on any failure: the version is optional and never affects the result.
///
/// Only that one flag is tried. The tool gets no stdin and is killed after `timeout`.
pub fn probe(program: &Path, timeout: Duration) -> Option<String> {
    let limits = Limits {
        timeout,
        max_output: MAX_OUTPUT,
        capture_stderr: true,
    };
    let captured = match process::run(Command::new(program).arg("--version"), limits) {
        Ok(captured) => captured,
        Err(error) => {
            debug!(
                "version probe of {} failed to start: {error}",
                program.display()
            );
            return None;
        }
    };
    match captured.outcome {
        Outcome::TimedOut => {
            debug!("version probe of {} timed out", program.display());
            return None;
        }
        Outcome::Exited(status) if !status.success() => {
            debug!(
                "version probe of {} exited with {status}",
                program.display()
            );
            return None;
        }
        // A flood still starts with the version line, if there is one.
        Outcome::Exited(_) | Outcome::Flooded => {}
    }
    first_line(&captured.stdout).or_else(|| first_line(&captured.stderr))
}

/// First non-empty line of `bytes`, trimmed and shortened.
fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_LINE_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_skips_blanks_and_trims() {
        assert_eq!(
            first_line(b"\n  \n  tool 1.2.3  \nsecond\n").as_deref(),
            Some("tool 1.2.3")
        );
        assert_eq!(first_line(b""), None);
        assert_eq!(first_line(b" \n\n"), None);
    }

    #[test]
    fn first_line_is_shortened() {
        let long = "x".repeat(500);
        assert_eq!(
            first_line(long.as_bytes()).unwrap().chars().count(),
            MAX_LINE_CHARS
        );
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        assert!(first_line(&[0xff, 0xfe, b'a']).is_some());
    }
}

#[cfg(all(test, unix))]
mod fake_tool_tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;

    /// Writes an executable shell script `name` into `dir`.
    fn fake_tool(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn version_from_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(dir.path(), "t", "echo; echo 'fake 1.2.3'; echo more");
        assert_eq!(probe(&tool, DEFAULT_TIMEOUT).as_deref(), Some("fake 1.2.3"));
    }

    #[test]
    fn version_falls_back_to_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(dir.path(), "t", "echo 'fake 9' >&2");
        assert_eq!(probe(&tool, DEFAULT_TIMEOUT).as_deref(), Some("fake 9"));
    }

    #[test]
    fn only_the_version_flag_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(
            dir.path(),
            "t",
            r#"[ "$1" = --version ] && [ $# -eq 1 ] && echo ok"#,
        );
        assert_eq!(probe(&tool, DEFAULT_TIMEOUT).as_deref(), Some("ok"));
    }

    #[test]
    fn no_output_or_failure_means_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let silent = fake_tool(dir.path(), "silent", "true");
        let failing = fake_tool(dir.path(), "failing", "echo 'unknown option' >&2; exit 2");
        assert_eq!(probe(&silent, DEFAULT_TIMEOUT), None);
        assert_eq!(probe(&failing, DEFAULT_TIMEOUT), None);
        assert_eq!(probe(Path::new("/nonexistent/tool"), DEFAULT_TIMEOUT), None);
    }

    #[test]
    fn stdin_is_closed() {
        let dir = tempfile::tempdir().unwrap();
        // `cat` returns at once on a closed stdin; on an inherited one it would block.
        let tool = fake_tool(dir.path(), "t", "cat; echo done");
        assert_eq!(
            probe(&tool, Duration::from_secs(2)).as_deref(),
            Some("done")
        );
    }

    #[test]
    fn slow_tool_is_killed_within_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let tool = fake_tool(dir.path(), "t", "sleep 10; echo late");
        let started = Instant::now();
        assert_eq!(probe(&tool, Duration::from_millis(300)), None);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn huge_output_is_handled() {
        let dir = tempfile::tempdir().unwrap();
        let endless = fake_tool(dir.path(), "endless", "echo 'first line'; yes");
        let started = Instant::now();
        assert_eq!(
            probe(&endless, Duration::from_secs(5)).as_deref(),
            Some("first line")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let big = fake_tool(
            dir.path(),
            "big",
            "head -c 5000000 /dev/zero | tr '\\0' 'a'",
        );
        let version = probe(&big, Duration::from_secs(5)).unwrap();
        assert_eq!(version.chars().count(), MAX_LINE_CHARS);
    }
}
