//! Git-style automatic pager: the report goes through a pager only when stdout is a terminal.

use std::ffi::OsString;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use log::debug;

/// Pager used when neither `$CHECKHEALTH_PAGER` nor `$PAGER` is set. `-F` makes `less` quit at
/// once when the output fits one screen, `-R` keeps colours and `-X` leaves the screen as is.
const DEFAULT_PAGER: &str = "less -FRX";

/// A pager command split into program and arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct PagerCommand {
    /// Program to run.
    pub program: String,
    /// Arguments passed to it.
    pub args: Vec<String>,
}

/// Picks the pager command: `checkhealth_pager`, then `pager`, then `less -FRX`. Empty values
/// are ignored. A `less` pager always gets `-R` so colours survive.
pub fn select(checkhealth_pager: Option<&str>, pager: Option<&str>) -> PagerCommand {
    let chosen = [checkhealth_pager, pager]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PAGER);
    let mut words = chosen.split_whitespace().map(str::to_string);
    let program = words.next().unwrap_or_else(|| "less".to_string());
    let mut args: Vec<String> = words.collect();
    if is_less(&program) && !args.iter().any(|arg| asks_for_raw_colours(arg)) {
        args.push("-R".to_string());
    }
    PagerCommand { program, args }
}

/// Whether `program` is `less` (with or without a directory or a `.exe` suffix).
fn is_less(program: &str) -> bool {
    Path::new(program)
        .file_stem()
        .is_some_and(|name| name.eq_ignore_ascii_case("less"))
}

/// Whether a `less` argument already contains the `-R` (or `-r`) option.
fn asks_for_raw_colours(arg: &str) -> bool {
    arg == "--RAW-CONTROL-CHARS"
        || arg == "--raw-control-chars"
        || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains(['R', 'r']))
}

/// Whether to page: only on a terminal, and not with `--no-pager` or `$NO_PAGER`.
pub fn enabled(no_pager_flag: bool, stdout_is_terminal: bool) -> bool {
    enabled_with(
        no_pager_flag,
        std::env::var_os("NO_PAGER"),
        stdout_is_terminal,
    )
}

/// [`enabled`] with the `$NO_PAGER` value passed in. An empty value does not count.
fn enabled_with(
    no_pager_flag: bool,
    no_pager_env: Option<OsString>,
    stdout_is_terminal: bool,
) -> bool {
    stdout_is_terminal && !no_pager_flag && no_pager_env.is_none_or(|value| value.is_empty())
}

/// Writes `report` to stdout, through the pager when `page` is true.
///
/// If the pager cannot be started, the report is printed directly. A pager the user quits early
/// is not an error.
pub fn output(report: &[u8], page: bool) -> io::Result<()> {
    if page {
        let command = select(
            std::env::var("CHECKHEALTH_PAGER").ok().as_deref(),
            std::env::var("PAGER").ok().as_deref(),
        );
        if write_to_pager(&command, report) {
            return Ok(());
        }
    }
    print_directly(report)
}

/// Prints `report` to stdout. A closed pipe is not an error.
fn print_directly(report: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    ignore_broken_pipe(stdout.write_all(report).and_then(|()| stdout.flush()))
}

/// Feeds `report` to the pager and waits for it to exit. Returns false when the pager could not
/// be started, so the caller can print directly instead.
fn write_to_pager(command: &PagerCommand, report: &[u8]) -> bool {
    let spawned = Command::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            debug!("cannot start pager `{}`: {error}", command.program);
            return false;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = ignore_broken_pipe(stdin.write_all(report)) {
            debug!("cannot write to the pager: {error}");
        }
        // Dropping stdin closes the pipe, so the pager sees the end of the input.
    }
    if let Err(error) = child.wait() {
        debug!("cannot wait for the pager: {error}");
    }
    true
}

/// Turns `BrokenPipe` (the reader quit early) into success.
fn ignore_broken_pipe(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(program: &str, args: &[&str]) -> PagerCommand {
        PagerCommand {
            program: program.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    #[test]
    fn defaults_to_less() {
        assert_eq!(select(None, None), command("less", &["-FRX"]));
    }

    #[test]
    fn checkhealth_pager_beats_pager() {
        assert_eq!(
            select(Some("bat --paging=always"), Some("most")),
            command("bat", &["--paging=always"])
        );
    }

    #[test]
    fn pager_is_used_without_checkhealth_pager() {
        assert_eq!(select(None, Some("most")), command("most", &[]));
    }

    #[test]
    fn empty_values_are_ignored() {
        assert_eq!(select(Some("  "), Some("most")), command("most", &[]));
        assert_eq!(select(Some(""), Some("")), command("less", &["-FRX"]));
    }

    #[test]
    fn less_always_gets_raw_colours() {
        assert_eq!(select(None, Some("less")), command("less", &["-R"]));
        assert_eq!(
            select(None, Some("/usr/bin/less -F")),
            command("/usr/bin/less", &["-F", "-R"])
        );
        assert_eq!(select(None, Some("less -FRX")), command("less", &["-FRX"]));
        assert_eq!(
            select(None, Some("less.exe -r")),
            command("less.exe", &["-r"])
        );
    }

    #[test]
    fn pages_only_on_a_terminal_without_opt_out() {
        assert!(enabled_with(false, None, true));
        assert!(!enabled_with(false, None, false));
        assert!(!enabled_with(true, None, true));
        assert!(!enabled_with(false, Some("1".into()), true));
        assert!(enabled_with(false, Some("".into()), true));
    }
}
