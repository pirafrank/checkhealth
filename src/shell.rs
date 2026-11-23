//! Shell selection. Resolution only; running a shell lives further down this module's API.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::error::{ExitCode, WithExitCode};

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
}
