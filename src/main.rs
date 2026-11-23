//! checkhealth: check whether the tools in your toolbelt are installed.

#![warn(missing_docs)]

mod check;
mod cli;
mod completions;
mod config;
mod constants;
mod error;
mod process;
mod report;
mod shell;
mod version;

use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use log::{LevelFilter, debug};

use crate::check::{CheckResult, SearchEnv, Status, check_command, plan};
use crate::cli::{Cli, ColorWhen, Commands};
use crate::config::Config;
use crate::error::{ExitCode, exit_code_of};
use crate::shell::ShellSpec;

/// Set when debug logging is on, so the panic hook can add the location.
static DEBUG_OUTPUT: AtomicBool = AtomicBool::new(false);

/// Builds the message printed for a panic (without the `error: ` prefix).
fn panic_message(payload: &str, location: Option<&str>, debug_output: bool) -> String {
    let mut message = format!("internal error: {payload}");
    if let (true, Some(location)) = (debug_output, location) {
        message.push_str(&format!(" (at {location})"));
    }
    message.push_str(&format!(
        "\nThis is a bug in checkhealth, please report it at {}/issues",
        env!("CARGO_PKG_REPOSITORY")
    ));
    message
}

/// Reports a panic and exits with [`ExitCode::Internal`].
///
/// The hook exits the process directly, so a panic on a worker thread cannot leave the run
/// hanging or finishing with a success code.
fn panic_hook(info: &PanicHookInfo<'_>) {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()));
    let message = panic_message(
        payload,
        location.as_deref(),
        DEBUG_OUTPUT.load(Ordering::Relaxed),
    );
    report::print_error(&message, ColorWhen::Auto);
    std::process::exit(i32::from(ExitCode::Internal.code()));
}

/// Runs the requested action and returns the exit code of a completed run.
fn run(cli: Cli) -> Result<ExitCode> {
    if let Some(Commands::Completions { shell }) = cli.command {
        completions::generate_completions(shell, &mut std::io::stdout());
        return Ok(ExitCode::Ok);
    }

    let args = cli.check;
    let config_path = args
        .config
        .expect("clap requires --config without a subcommand");
    let config = Config::load(&config_path)?;
    debug!("Loaded config: {config:?}");

    #[cfg(feature = "test-hooks")]
    if std::env::var_os("CHECKHEALTH_TEST_PANIC").is_some() {
        panic!("induced by CHECKHEALTH_TEST_PANIC");
    }

    let shell = ShellSpec::resolve_current(args.shell.as_deref())?;
    if args.shell.is_some() {
        shell.ensure_exists()?;
    }
    debug!("Shell: {shell:?}");

    let probe_timeout = (!args.no_version).then(|| {
        args.timeout
            .map_or(version::DEFAULT_TIMEOUT, Duration::from_secs_f64)
    });
    let search_env = SearchEnv::from_process();
    let results: Vec<CheckResult> = plan(&config)
        .into_iter()
        .map(|entry| {
            let status = match entry.kind {
                check::CheckKind::Command => {
                    let mut status = check_command(&search_env, &entry.name);
                    if let (
                        Status::Found {
                            path: Some(path),
                            version,
                        },
                        Some(timeout),
                    ) = (&mut status, probe_timeout)
                    {
                        *version = version::probe(path, timeout);
                    }
                    status
                }
                _ => Status::Skipped("not implemented yet".to_string()),
            };
            CheckResult { entry, status }
        })
        .collect();

    report::print(&results, args.color).context("failed to write the report")?;

    let any_missing = results.iter().any(|r| r.status == Status::Missing);
    Ok(if any_missing {
        ExitCode::Missing
    } else {
        ExitCode::Ok
    })
}

/// Runs the tool and maps every outcome to an exit code. The only place that does so.
fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(panic_hook));

    let cli = Cli::parse();
    let level = cli.verbose.log_level_filter();
    DEBUG_OUTPUT.store(level >= LevelFilter::Debug, Ordering::Relaxed);
    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();

    let color = cli.check.color;
    match run(cli) {
        Ok(code) => code.into(),
        Err(error) => {
            report::print_error(&format!("{error:#}"), color);
            exit_code_of(&error).into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_message_hides_location_unless_debugging() {
        let quiet = panic_message("boom", Some("src/x.rs:1"), false);
        assert!(quiet.starts_with("internal error: boom\n"));
        assert!(!quiet.contains("src/x.rs"));
        assert!(quiet.contains("/issues"));
        let verbose = panic_message("boom", Some("src/x.rs:1"), true);
        assert!(verbose.starts_with("internal error: boom (at src/x.rs:1)"));
    }
}
