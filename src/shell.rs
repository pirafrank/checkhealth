//! Shell selection and the single shell invocation that answers all alias and function
//! questions.
//!
//! The shell is started once as `<shell> -i -c '<fixed script>' <shell> a:<name>... f:<name>...`
//! with stdin closed. `-i` makes it load the rc file that defines aliases and functions. Names
//! travel as positional arguments (PowerShell: an environment variable), never inside the
//! script text. The script prints one marker line per name and only marker lines are parsed, so
//! anything the rc files print is ignored.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use log::{Level, debug, log_enabled, warn};

use crate::check::{CheckKind, Entry, Status};
use crate::error::{ExitCode, WithExitCode};
use crate::process::{self, Captured, Limits, Outcome};
use crate::runner::ShellChecker;

/// Names accepted by `--shell`, for error messages.
const SUPPORTED: &str = "zsh, bash, sh, cmd, powershell, pwsh";

/// A shell checkhealth knows how to talk to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// Z shell.
    Zsh,
    /// GNU Bash.
    Bash,
    /// POSIX `sh`. Cannot answer alias or function questions.
    Sh,
    /// Windows `cmd.exe`. Cannot answer alias or function questions.
    Cmd,
    /// Windows PowerShell 5 (`powershell.exe`).
    Powershell,
    /// PowerShell 7+ (`pwsh`).
    Pwsh,
}

impl Shell {
    /// Maps a shell name or path (`zsh`, `/bin/bash`, `pwsh.exe`) to a shell kind.
    pub fn from_name(name: &str) -> Option<Self> {
        let file = Path::new(name).file_name()?.to_str()?.to_ascii_lowercase();
        let stem = file.strip_suffix(".exe").unwrap_or(&file);
        match stem {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "sh" => Some(Self::Sh),
            "cmd" => Some(Self::Cmd),
            "powershell" => Some(Self::Powershell),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// Program name used when the shell was not given explicitly.
    fn default_program(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::Sh => "sh",
            Self::Cmd => "cmd.exe",
            Self::Powershell => "powershell.exe",
            Self::Pwsh => "pwsh",
        }
    }
}

/// A shell to run: its kind and the program (a bare name or a path) that starts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellSpec {
    /// What kind of shell this is.
    pub kind: Shell,
    /// Program to execute: a bare name looked up on `PATH`, or a path.
    pub program: PathBuf,
}

impl ShellSpec {
    /// Resolves the shell to use.
    ///
    /// 1. `requested` (`--shell`), a bare name or a path.
    /// 2. On Windows, `powershell.exe`.
    /// 3. On Unix, `zsh` if `login_shell` (`$SHELL`) contains "zsh", `bash` if it is set to
    ///    anything else, `sh` if it is unset.
    pub fn resolve(
        requested: Option<&str>,
        login_shell: Option<&OsStr>,
        windows: bool,
    ) -> Result<Self> {
        if let Some(name) = requested {
            let Some(kind) = Shell::from_name(name) else {
                return Err(anyhow::anyhow!(
                    "unsupported shell `{name}` (supported: {SUPPORTED})"
                ))
                .exit_code(ExitCode::Shell);
            };
            return Ok(Self {
                kind,
                program: PathBuf::from(name),
            });
        }
        let kind = if windows {
            Shell::Powershell
        } else {
            match login_shell {
                None => Shell::Sh,
                Some(value) if value.to_string_lossy().contains("zsh") => Shell::Zsh,
                Some(_) => Shell::Bash,
            }
        };
        Ok(Self {
            kind,
            program: PathBuf::from(kind.default_program()),
        })
    }

    /// Resolves the shell for the current platform and environment.
    pub fn resolve_current(requested: Option<&str>) -> Result<Self> {
        let login_shell = std::env::var_os("SHELL");
        Self::resolve(requested, login_shell.as_deref(), cfg!(windows))
    }

    /// Fails with exit code 4 if the program cannot be found.
    pub fn ensure_exists(&self) -> Result<()> {
        let found = if self.program.components().count() > 1 {
            self.program.is_file()
        } else {
            which::which(&self.program).is_ok()
        };
        let result = if found {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "shell `{}` not found",
                self.program.display()
            ))
        };
        result.exit_code(ExitCode::Shell)
    }
}

/// Default time the shell gets to start and answer.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Most stdout kept from the shell. The answers come last, after whatever the rc files print.
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// Prefix of the lines the fixed scripts print.
const MARKER: &str = "@@checkhealth@@";
/// Environment variable carrying the names to PowerShell.
const ENTRIES_VAR: &str = "CHECKHEALTH_ENTRIES";

/// bash: `$1...` are `a:<alias>` or `f:<function>`.
const BASH_SCRIPT: &str = r#"for spec in "$@"; do
  kind=${spec%%:*}; name=${spec#*:}; found=0
  case $kind in
    a) alias -- "$name" >/dev/null 2>&1 && found=1;;
    f) declare -F -- "$name" >/dev/null 2>&1 && found=1;;
  esac
  echo "@@checkhealth@@|$spec|$found"
done"#;

/// zsh: same protocol as bash. `(e)` makes the subscript an exact key, not a pattern.
const ZSH_SCRIPT: &str = r#"for spec in "$@"; do
  kind=${spec%%:*}; name=${spec#*:}; found=0
  case $kind in
    a) (( ${+aliases[(e)$name]} )) && found=1;;
    f) (( ${+functions[(e)$name]} )) && found=1;;
  esac
  echo "@@checkhealth@@|$spec|$found"
done"#;

/// PowerShell: entries come from `$env:CHECKHEALTH_ENTRIES`, one `a:<name>` or `f:<name>` per
/// line. Single quotes only, so Windows command-line quoting cannot alter it.
const POWERSHELL_SCRIPT: &str = "foreach ($e in ($env:CHECKHEALTH_ENTRIES -split [char]10)) { \
    if ($e -eq '') { continue }; $n = $e.Substring(2); \
    if ($e.StartsWith('a')) { $found = [bool](Get-Alias -Name $n -ErrorAction SilentlyContinue) } \
    else { $found = [bool](Get-Command -CommandType Function -Name $n -ErrorAction SilentlyContinue) }; \
    Write-Output ('@@checkhealth@@|' + $e + '|' + [int]$found) }";

/// Runs the chosen shell once for all aliases and functions.
pub struct ShellRunner {
    /// The shell to start.
    pub spec: ShellSpec,
    /// How long it gets to answer.
    pub timeout: Duration,
}

/// The argument naming `entry` for the fixed scripts: `a:<name>` or `f:<name>`.
fn spec_of(entry: &Entry) -> String {
    let kind = if entry.kind == CheckKind::Alias {
        'a'
    } else {
        'f'
    };
    format!("{kind}:{}", entry.name)
}

impl ShellRunner {
    /// Why the shell cannot answer at all, if so. Such entries are `skipped`, never `missing`.
    fn unsupported_reason(&self) -> Option<&'static str> {
        match self.spec.kind {
            Shell::Sh => Some("not supported on sh"),
            Shell::Cmd => Some("not supported on cmd.exe"),
            _ => None,
        }
    }

    /// Builds the command that asks the shell about `entries`.
    fn command(&self, entries: &[Entry]) -> Command {
        let mut command = Command::new(&self.spec.program);
        let specs = entries.iter().map(spec_of);
        match self.spec.kind {
            Shell::Bash | Shell::Zsh => {
                let script = if self.spec.kind == Shell::Bash {
                    BASH_SCRIPT
                } else {
                    ZSH_SCRIPT
                };
                command
                    .args(["-i", "-c", script, "checkhealth"])
                    .args(specs);
            }
            _ => {
                let joined = specs.collect::<Vec<_>>().join("\n");
                command
                    .args(["-NoLogo", "-NonInteractive", "-Command", POWERSHELL_SCRIPT])
                    .env(ENTRIES_VAR, joined);
            }
        }
        command
    }
}

impl ShellChecker for ShellRunner {
    fn check(&self, entries: &[Entry]) -> Result<Vec<Status>> {
        if let Some(reason) = self.unsupported_reason() {
            return Ok(vec![Status::Skipped(reason.to_string()); entries.len()]);
        }
        let limits = Limits {
            timeout: self.timeout,
            max_output: MAX_OUTPUT,
            capture_stderr: log_enabled!(Level::Debug),
        };
        debug!(
            "starting {} for {} alias/function names",
            self.spec.program.display(),
            entries.len()
        );
        let captured = process::run(&mut self.command(entries), limits);
        statuses_from(&self.spec, entries, captured)
    }
}

/// Turns the outcome of the shell invocation into one status per entry.
///
/// A shell that cannot be started is fatal (exit code 4). A timeout or a shell that does not
/// answer is not: the affected entries are skipped.
fn statuses_from(
    spec: &ShellSpec,
    entries: &[Entry],
    captured: io::Result<Captured>,
) -> Result<Vec<Status>> {
    let program = spec.program.display();
    let captured = captured
        .with_context(|| format!("failed to start shell `{program}`"))
        .exit_code(ExitCode::Shell)?;
    if !captured.stderr.is_empty() {
        debug!(
            "shell stderr: {}",
            String::from_utf8_lossy(&captured.stderr).trim_end()
        );
    }
    let skip_all = |reason: &str| vec![Status::Skipped(reason.to_string()); entries.len()];
    match captured.outcome {
        Outcome::TimedOut => {
            warn!("shell `{program}` timed out; aliases and functions are skipped");
            return Ok(skip_all("shell timed out"));
        }
        Outcome::Flooded => {
            warn!("shell `{program}` printed too much output; aliases and functions are skipped");
            return Ok(skip_all("shell output too large"));
        }
        Outcome::Exited(_) => {}
    }
    let answers = parse_answers(&String::from_utf8_lossy(&captured.stdout));
    let mut unanswered = false;
    let statuses = entries
        .iter()
        .map(|entry| match answers.get(&spec_of(entry)) {
            Some(true) => Status::Found {
                path: None,
                version: None,
            },
            Some(false) => Status::Missing,
            None => {
                unanswered = true;
                Status::Skipped("no answer from the shell".to_string())
            }
        })
        .collect();
    if unanswered {
        warn!("shell `{program}` did not answer for every name (does an rc file exit early?)");
    }
    Ok(statuses)
}

/// Reads the marker lines of the fixed scripts: `<spec>` maps to whether it was found.
fn parse_answers(stdout: &str) -> HashMap<String, bool> {
    let prefix = format!("{MARKER}|");
    stdout
        .lines()
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix(prefix.as_str()))
        .filter_map(|rest| {
            let (spec, flag) = rest.rsplit_once('|')?;
            Some((spec.to_string(), flag == "1"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_unix(requested: Option<&str>, login_shell: Option<&str>) -> Result<ShellSpec> {
        ShellSpec::resolve(requested, login_shell.map(OsStr::new), false)
    }

    #[test]
    fn explicit_shell_accepts_names_and_paths() {
        assert_eq!(resolve_unix(Some("zsh"), None).unwrap().kind, Shell::Zsh);
        let spec = resolve_unix(Some("/usr/bin/bash"), None).unwrap();
        assert_eq!(spec.kind, Shell::Bash);
        assert_eq!(spec.program, PathBuf::from("/usr/bin/bash"));
        assert_eq!(
            resolve_unix(Some("PWSH.EXE"), None).unwrap().kind,
            Shell::Pwsh
        );
        assert_eq!(resolve_unix(Some("cmd"), None).unwrap().kind, Shell::Cmd);
        assert_eq!(
            resolve_unix(Some("powershell.exe"), None).unwrap().kind,
            Shell::Powershell
        );
        assert_eq!(resolve_unix(Some("sh"), None).unwrap().kind, Shell::Sh);
    }

    #[test]
    fn explicit_shell_wins_over_environment() {
        assert_eq!(
            resolve_unix(Some("sh"), Some("/bin/zsh")).unwrap().kind,
            Shell::Sh
        );
    }

    #[test]
    fn unix_default_follows_login_shell() {
        assert_eq!(
            resolve_unix(None, Some("/usr/bin/zsh")).unwrap().kind,
            Shell::Zsh
        );
        assert_eq!(
            resolve_unix(None, Some("/bin/bash")).unwrap().kind,
            Shell::Bash
        );
        assert_eq!(
            resolve_unix(None, Some("/usr/bin/fish")).unwrap().kind,
            Shell::Bash
        );
        assert_eq!(resolve_unix(None, None).unwrap().kind, Shell::Sh);
    }

    #[test]
    fn windows_default_is_powershell() {
        let spec = ShellSpec::resolve(None, Some(OsStr::new("/bin/zsh")), true).unwrap();
        assert_eq!(spec.kind, Shell::Powershell);
        assert_eq!(spec.program, PathBuf::from("powershell.exe"));
    }

    #[test]
    fn unsupported_shell_is_a_shell_error() {
        let error = resolve_unix(Some("fish"), None).unwrap_err();
        assert_eq!(crate::error::exit_code_of(&error), ExitCode::Shell);
        let message = format!("{error:#}");
        assert!(
            message.contains("`fish`") && message.contains("zsh, bash"),
            "{message}"
        );
    }

    #[test]
    fn missing_shell_program_is_a_shell_error() {
        let spec = ShellSpec {
            kind: Shell::Bash,
            program: PathBuf::from("/nonexistent/bash"),
        };
        let error = spec.ensure_exists().unwrap_err();
        assert_eq!(crate::error::exit_code_of(&error), ExitCode::Shell);
    }

    fn entry(kind: CheckKind, name: &str) -> Entry {
        Entry {
            kind,
            name: name.to_string(),
        }
    }

    fn bash() -> ShellSpec {
        ShellSpec {
            kind: Shell::Bash,
            program: PathBuf::from("bash"),
        }
    }

    fn captured(stdout: &str, outcome: Outcome) -> io::Result<Captured> {
        Ok(Captured {
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            outcome,
        })
    }

    #[cfg(unix)]
    fn exited() -> Outcome {
        use std::os::unix::process::ExitStatusExt;
        Outcome::Exited(std::process::ExitStatus::from_raw(0))
    }

    #[test]
    fn answers_are_read_from_marker_lines_only() {
        let output = "To run a command as administrator...\r\n@@checkhealth@@|a:ll|1\r\n\
                      garbage|a:fake|1\n@@checkhealth@@|f:yy|0\n@@checkhealth@@|f:odd|name|1\n";
        let answers = parse_answers(output);
        assert_eq!(answers.get("a:ll"), Some(&true));
        assert_eq!(answers.get("f:yy"), Some(&false));
        assert_eq!(answers.get("f:odd|name"), Some(&true));
        assert_eq!(answers.get("a:fake"), None);
    }

    #[cfg(unix)]
    #[test]
    fn statuses_follow_the_answers_and_missing_answers_are_skipped() {
        let entries = [
            entry(CheckKind::Alias, "ll"),
            entry(CheckKind::Alias, "nope"),
            entry(CheckKind::Function, "yy"),
            entry(CheckKind::Function, "silent"),
        ];
        let out = "@@checkhealth@@|a:ll|1\n@@checkhealth@@|a:nope|0\n@@checkhealth@@|f:yy|1\n";
        let statuses = statuses_from(&bash(), &entries, captured(out, exited())).unwrap();
        assert_eq!(
            statuses[0],
            Status::Found {
                path: None,
                version: None
            }
        );
        assert_eq!(statuses[1], Status::Missing);
        assert_eq!(
            statuses[2],
            Status::Found {
                path: None,
                version: None
            }
        );
        assert!(matches!(statuses[3], Status::Skipped(_)));
    }

    #[test]
    fn a_timeout_skips_every_entry_without_failing() {
        let entries = [
            entry(CheckKind::Alias, "ll"),
            entry(CheckKind::Function, "yy"),
        ];
        let statuses = statuses_from(&bash(), &entries, captured("", Outcome::TimedOut)).unwrap();
        assert_eq!(statuses, vec![Status::Skipped("shell timed out".into()); 2]);
    }

    #[test]
    fn a_flooding_shell_skips_every_entry() {
        let entries = [entry(CheckKind::Alias, "ll")];
        let statuses = statuses_from(&bash(), &entries, captured("", Outcome::Flooded)).unwrap();
        assert!(matches!(statuses[0], Status::Skipped(_)));
    }

    #[test]
    fn a_shell_that_cannot_start_is_a_shell_error() {
        let error = statuses_from(
            &bash(),
            &[entry(CheckKind::Alias, "ll")],
            Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        )
        .unwrap_err();
        assert_eq!(crate::error::exit_code_of(&error), ExitCode::Shell);
        assert!(format!("{error:#}").contains("failed to start shell `bash`"));
    }

    #[test]
    fn sh_and_cmd_are_skipped_without_starting_anything() {
        let entries = [
            entry(CheckKind::Alias, "ll"),
            entry(CheckKind::Function, "yy"),
        ];
        for (kind, text) in [(Shell::Sh, "sh"), (Shell::Cmd, "cmd.exe")] {
            let runner = ShellRunner {
                spec: ShellSpec {
                    kind,
                    program: PathBuf::from("/nonexistent/shell"),
                },
                timeout: DEFAULT_TIMEOUT,
            };
            let statuses = runner.check(&entries).unwrap();
            assert_eq!(
                statuses,
                vec![Status::Skipped(format!("not supported on {text}")); 2]
            );
        }
    }

    #[test]
    fn names_are_arguments_never_part_of_the_script() {
        let entries = [
            entry(CheckKind::Alias, "ll"),
            entry(CheckKind::Function, "a:b"),
        ];
        for kind in [Shell::Bash, Shell::Zsh] {
            let runner = ShellRunner {
                spec: ShellSpec {
                    kind,
                    program: PathBuf::from("x"),
                },
                timeout: DEFAULT_TIMEOUT,
            };
            let command = runner.command(&entries);
            let args: Vec<_> = command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert_eq!(args[..2], ["-i", "-c"]);
            assert_eq!(args[3..], ["checkhealth", "a:ll", "f:a:b"]);
        }
    }

    #[test]
    fn powershell_receives_names_in_the_environment() {
        let runner = ShellRunner {
            spec: ShellSpec {
                kind: Shell::Pwsh,
                program: PathBuf::from("pwsh"),
            },
            timeout: DEFAULT_TIMEOUT,
        };
        let command = runner.command(&[
            entry(CheckKind::Alias, "ll"),
            entry(CheckKind::Function, "yy"),
        ]);
        let var = command
            .get_envs()
            .find(|(k, _)| *k == OsStr::new(ENTRIES_VAR))
            .unwrap();
        assert_eq!(var.1, Some(OsStr::new("a:ll\nf:yy")));
        assert!(!POWERSHELL_SCRIPT.contains('"'));
    }
}
