//! Exit codes and the error type that carries them.
//!
//! Every failure class has its own exit code (part of the public interface, keep the numbers
//! stable). Fallible code attaches a code with [`AppError`] through `anyhow`; `main` is the
//! only place that maps an error back to a code.

use std::error::Error;
use std::fmt;

/// Process exit codes. The numbers are part of the public interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    /// All checked entries were found. Skipped entries do not count as missing.
    Ok = 0,
    /// The run completed and at least one entry is missing.
    Missing = 1,
    /// Invalid command line. Produced by clap, which already exits with 2.
    Usage = 2,
    /// Config file missing, unreadable, invalid YAML, unknown key or invalid name.
    Config = 3,
    /// The requested or default shell is unsupported or cannot be started at all.
    Shell = 4,
    /// Any other unexpected error, including panics.
    Internal = 5,
}

impl ExitCode {
    /// Numeric value passed to the operating system.
    pub fn code(self) -> u8 {
        self as u8
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code.code())
    }
}

/// An error tagged with the exit code it maps to.
///
/// It displays as the wrapped error and forwards its `source` chain, so printing the
/// `anyhow` chain shows each message exactly once.
#[derive(Debug)]
pub struct AppError {
    /// Exit code this failure maps to.
    kind: ExitCode,
    /// The underlying error with its context chain.
    source: anyhow::Error,
}

impl AppError {
    /// Tags `source` with `kind`.
    pub fn new(kind: ExitCode, source: anyhow::Error) -> Self {
        Self { kind, source }
    }

    /// The exit code of this error.
    pub fn kind(&self) -> ExitCode {
        self.kind
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, f)
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.chain().nth(1)
    }
}

/// Extension to tag an `anyhow` result with an exit code.
pub trait WithExitCode<T> {
    /// Marks the error, if any, as mapping to `kind`.
    fn exit_code(self, kind: ExitCode) -> anyhow::Result<T>;
}

impl<T> WithExitCode<T> for anyhow::Result<T> {
    fn exit_code(self, kind: ExitCode) -> anyhow::Result<T> {
        self.map_err(|source| anyhow::Error::new(AppError::new(kind, source)))
    }
}

/// Finds the exit code attached to `error`, defaulting to [`ExitCode::Internal`].
pub fn exit_code_of(error: &anyhow::Error) -> ExitCode {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AppError>())
        .map_or(ExitCode::Internal, AppError::kind)
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, anyhow};

    use super::*;

    #[test]
    fn numbers_are_stable() {
        let codes = [
            (ExitCode::Ok, 0),
            (ExitCode::Missing, 1),
            (ExitCode::Usage, 2),
            (ExitCode::Config, 3),
            (ExitCode::Shell, 4),
            (ExitCode::Internal, 5),
        ];
        for (code, number) in codes {
            assert_eq!(code.code(), number);
        }
    }

    #[test]
    fn untagged_errors_are_internal() {
        assert_eq!(exit_code_of(&anyhow!("boom")), ExitCode::Internal);
    }

    #[test]
    fn tag_survives_added_context() {
        let result: anyhow::Result<()> = Err(anyhow!("root cause"));
        let error = result
            .exit_code(ExitCode::Shell)
            .context("outer")
            .unwrap_err();
        assert_eq!(exit_code_of(&error), ExitCode::Shell);
    }

    #[test]
    fn chain_prints_each_message_once() {
        let result: anyhow::Result<()> = Err(anyhow!("root cause"));
        let result = result.context("failed to read config x");
        let error = result.exit_code(ExitCode::Config).unwrap_err();
        assert_eq!(format!("{error:#}"), "failed to read config x: root cause");
    }
}
