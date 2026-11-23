//! Config file: structure, loading and validation.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};

use crate::error::{ExitCode, WithExitCode};

/// The names to check, as listed in the YAML config file.
///
/// All keys are optional and unknown keys are rejected, so a typo such as `command:` is an
/// error instead of being silently ignored.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Executables looked up on `PATH`.
    #[serde(default, deserialize_with = "list_or_null")]
    pub commands: Vec<String>,
    /// Shell aliases.
    #[serde(default, deserialize_with = "list_or_null")]
    pub aliases: Vec<String>,
    /// Shell functions.
    #[serde(default, deserialize_with = "list_or_null")]
    pub functions: Vec<String>,
}

/// Accepts a list, or a bare `key:` with no value, as an empty list.
fn list_or_null<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

impl Config {
    /// Parses and validates a config from YAML text. An empty document is an empty config.
    pub fn parse(text: &str) -> Result<Self> {
        let config: Option<Config> =
            serde_yaml_ng::from_str(text).context("invalid YAML or unknown key")?;
        let config = config.unwrap_or_default();
        config.validate()?;
        Ok(config)
    }

    /// Reads, parses and validates the config at `path`. Every failure maps to exit code 3.
    pub fn load(path: &Path) -> Result<Self> {
        let load = || -> Result<Self> {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            Self::parse(&text).with_context(|| format!("failed to parse config {}", path.display()))
        };
        load().exit_code(ExitCode::Config)
    }

    /// Checks that every name is safe to pass to a shell.
    fn validate(&self) -> Result<()> {
        let sections = [
            ("commands", &self.commands),
            ("aliases", &self.aliases),
            ("functions", &self.functions),
        ];
        for (section, names) in sections {
            for name in names {
                validate_name(name).with_context(|| format!("invalid entry in `{section}`"))?;
            }
        }
        Ok(())
    }
}

/// Checks a single name: non-empty, no whitespace or shell metacharacters, no leading `-`.
///
/// Letters, digits and `_ - . + @ : ,` are allowed. The leading `-` is rejected so a name can
/// never be taken for an option by the program that receives it.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("names must not be empty");
    }
    if name.starts_with('-') {
        bail!("`{name}` must not start with `-`");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '@' | ':' | ',')))
    {
        bail!("`{name}` contains {bad:?}, only letters, digits and `_-.+@:,` are allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn full_config_parses() {
        let config =
            Config::parse("commands: [cargo, git]\naliases: [ll]\nfunctions: [yy]\n").unwrap();
        assert_eq!(config.commands, names(&["cargo", "git"]));
        assert_eq!(config.aliases, names(&["ll"]));
        assert_eq!(config.functions, names(&["yy"]));
    }

    #[test]
    fn keys_default_to_empty() {
        let config = Config::parse("commands: [cargo]").unwrap();
        assert!(config.aliases.is_empty());
        assert!(config.functions.is_empty());
    }

    #[test]
    fn empty_document_and_null_keys_are_empty() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
        assert_eq!(
            Config::parse("# only a comment\n").unwrap(),
            Config::default()
        );
        assert_eq!(
            Config::parse("commands:\naliases:\n").unwrap(),
            Config::default()
        );
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let error = Config::parse("command: [cargo]").unwrap_err();
        assert!(format!("{error:#}").contains("command"));
    }

    #[test]
    fn invalid_yaml_is_rejected() {
        assert!(Config::parse("commands: [unclosed").is_err());
        assert!(Config::parse("commands: just-a-string").is_err());
    }

    #[test]
    fn invalid_names_are_rejected() {
        for bad in [
            "", "a b", "a;b", "$(x)", "a|b", "a&b", "`x`", "a/b", "a\\b", "-rf", "a\nb", "a'b",
            "a\"b", "a*", "a>b", "~", "a#b", "!x",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn plausible_names_are_accepted() {
        for good in [
            "cargo",
            "g++",
            "python3.12",
            "paste-image",
            "my_func",
            "ns:cmd",
            "é",
        ] {
            assert!(validate_name(good).is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn invalid_name_error_names_the_section() {
        let error = Config::parse("aliases: [\"a b\"]").unwrap_err();
        assert!(format!("{error:#}").contains("`aliases`"));
    }

    #[test]
    fn load_reports_missing_file_as_config_error() {
        let error = Config::load(Path::new("/nonexistent/checkhealth.yml")).unwrap_err();
        assert_eq!(crate::error::exit_code_of(&error), ExitCode::Config);
        assert!(format!("{error:#}").starts_with("failed to read config /nonexistent"));
    }
}
