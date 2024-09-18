use clap::Parser;
use colored::*;
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use which::which;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE")]
    config: PathBuf,

    /// Specifies the shell to use
    #[arg(short, long, value_name = "SHELL")]
    shell: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "Vec::new")]
    commands: Vec<String>,
    #[serde(default = "Vec::new")]
    aliases: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let shell = args.shell.unwrap_or_else(|| get_default_shell());

    let file = File::open(&args.config)?;
    let reader = BufReader::new(file);
    let config: Config = serde_yml::from_reader(reader)?;

    for cmd in config.commands {
        check_cmd(&cmd, &shell);
    }

    Ok(())
}

fn check_cmd(cmd: &str, _shell: &str) {
    let path = if cfg!(target_os = "windows") {
        format!("{}\\{}.exe", std::env::var("PATH").unwrap_or_default(), cmd)
    } else {
        cmd.to_string()
    };

    match which(&path) {
        Ok(_) => println!("{} {}", "[✓]".green(), cmd),
        Err(_) => println!("{} {}", "[✗]".red(), cmd),
    }
}

fn get_default_shell() -> String {
    if cfg!(target_os = "windows") {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL")
            .map(|s| if s.contains("zsh") { "zsh" } else { "bash" })
            .unwrap_or("sh")
            .to_string()
    }
}
