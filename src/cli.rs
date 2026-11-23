//! Command line interface definition (clap).
//!
//! Kept in its own module so it can also be reused by `examples/gen_man.rs`
//! to generate man pages.

use crate::constants::*;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use clap_verbosity_flag::{InfoLevel, Verbosity};

/// Text appended to `--help`: exit codes and caveats. Keep in sync with the README.
const AFTER_HELP: &str = "\
Exit codes:
  0  all checked entries found (skipped entries do not count as missing)
  1  the run completed and at least one entry is missing
  2  invalid command line
  3  config file missing, unreadable, invalid YAML, unknown key or invalid name
  4  the requested or default shell is unsupported or cannot be started
  5  internal error

Notes:
  Aliases and functions are looked up by starting the shell once, so detection depends on
  your rc files being loadable by a non-login interactive shell. When a shell cannot answer
  (sh, cmd.exe) the entries are reported as skipped, not missing.
  Version probing runs `<tool> --version` for every tool found in the config. Use
  --no-version if you do not trust the config.";

/// When to colour the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Colour when stdout is a terminal (honours `NO_COLOR` and `CLICOLOR_FORCE`)
    #[default]
    Auto,
    /// Always colour
    Always,
    /// Never colour
    Never,
}

/// Output format of the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    /// Human-readable report (the default when stdout is a terminal)
    Text,
    /// Tab-separated values, one entry per line (the default when stdout is piped)
    Tsv,
    /// Comma-separated values (RFC 4180 quoting), same columns as `tsv`
    Csv,
    /// A JSON array with one object per entry
    Json,
}

/// Parses a positive, finite number of seconds (fractions allowed).
fn parse_seconds(value: &str) -> Result<f64, String> {
    let seconds: f64 = value
        .parse()
        .map_err(|_| format!("`{value}` is not a number"))?;
    if seconds.is_finite() && seconds > 0.0 && seconds <= 86_400.0 {
        Ok(seconds)
    } else {
        Err("must be greater than 0 and at most 86400".to_string())
    }
}

/// Check that the tools in your toolbelt are installed.
///
/// Reads a YAML file listing commands, shell aliases and shell functions, and prints a
/// pass/fail line for each one. Use `checkhealth completions --shell <SHELL>` to generate
/// shell completions.
#[derive(Parser)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION"),
    long_version = long_version(),
    subcommand_negates_reqs = true,
    help_template = "\n\n{name} - {about}\n\n\
        {usage-heading} {usage}\n\n\
        {all-args}{after-help}",
    after_help = format!("{AFTER_HELP}\n\n\
        For more information, visit: {}\n\
        If you encounter any issues, please report them at:\n{}/issues\n",
        env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_REPOSITORY")),
)]
pub struct Cli {
    /// Subcommand to execute. Without one, the config file is checked.
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Options of the default check action
    #[command(flatten)]
    pub check: CheckArgs,

    /// Control verbosity level (use -v, -vv, -vvv, or -vvvv for more verbose output)
    #[command(flatten)]
    pub verbose: Verbosity<InfoLevel>,
}

/// Arguments of the default check action.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Path to the YAML config file
    #[arg(short, long, value_name = "FILE", required = true)]
    pub config: Option<PathBuf>,

    /// Shell used to check aliases and functions (a name such as `zsh`, or a path).
    /// Defaults to powershell.exe on Windows, and to zsh or bash (from $SHELL) elsewhere.
    /// Supported: zsh, bash, sh, cmd.exe, powershell.exe, pwsh.exe
    #[arg(short, long, value_name = "SHELL")]
    pub shell: Option<String>,

    /// When to colour the output
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,

    /// Output format. Defaults to `text` on a terminal and to `tsv` when stdout is piped.
    /// TSV and CSV columns: kind, name, status (found|missing|skipped), path, version, reason
    #[arg(short, long, value_enum, value_name = "FORMAT")]
    pub export: Option<ExportFormat>,

    /// Do not run tools to read their version
    #[arg(long)]
    pub no_version: bool,

    /// Per-process timeout in seconds for version probes and the shell
    /// (defaults: 2 s for probes, 5 s for the shell)
    #[arg(long, value_name = "SECONDS", value_parser = parse_seconds)]
    pub timeout: Option<f64>,
}

/// Available subcommands
#[derive(Subcommand)]
pub enum Commands {
    /// Generate shell completions to stdout
    Completions {
        /// Shell to generate completions for
        #[arg(long, short, value_enum)]
        shell: Shell,
    },
}

/// Returns a static string containing the version information.
/// It uses Box::leak to convert a String into a &'static str.
/// This is a workaround to avoid using a global static variable.
fn long_version() -> &'static str {
    #[cfg(static_linking)]
    let linking_type = "statically linked";
    #[cfg(dynamic_linking)]
    let linking_type = "dynamically linked";
    Box::leak(
        format!(
            "\nVersion   : {}\nCommit    : {}\nBuild Date: {}\nBuilt with: {} ({})",
            VERSION, COMMIT, BUILD_DATE, COMPILE_C_LIB, linking_type
        )
        .into_boxed_str(),
    )
}
