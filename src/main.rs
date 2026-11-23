//! checkhealth: check whether the tools in your toolbelt are installed.

#![warn(missing_docs)]

mod cli;
mod completions;
mod config;
mod constants;
mod error;

use anyhow::Result;
use clap::Parser;
use log::debug;

use crate::cli::{Cli, Commands};
use crate::config::Config;
use crate::error::{ExitCode, exit_code_of};

/// Parses arguments and runs the requested action.
fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.verbose.log_level_filter())
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();
    debug!("Debug logging enabled");

    if let Some(Commands::Completions { shell }) = cli.command {
        completions::generate_completions(shell, &mut std::io::stdout());
        return Ok(ExitCode::Ok);
    }

    let config_path = cli
        .check
        .config
        .expect("clap requires --config without a subcommand");
    let config = Config::load(&config_path)?;
    debug!("Loaded config: {config:?}");
    Ok(ExitCode::Ok)
}

/// Runs the tool and maps every outcome to an exit code. The only place that does so.
fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code.into(),
        Err(error) => {
            eprintln!("error: {error:#}");
            exit_code_of(&error).into()
        }
    }
}
