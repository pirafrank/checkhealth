//! What is checked and what came out of it.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::config::Config;

/// The kind of thing a name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    /// An executable on `PATH`.
    Command,
    /// A shell alias.
    Alias,
    /// A shell function.
    Function,
}

impl CheckKind {
    /// All kinds, in the order they are reported.
    pub const ALL: [CheckKind; 3] = [CheckKind::Command, CheckKind::Alias, CheckKind::Function];

    /// Title of the report group holding entries of this kind.
    pub fn heading(self) -> &'static str {
        match self {
            CheckKind::Command => "Commands",
            CheckKind::Alias => "Aliases",
            CheckKind::Function => "Functions",
        }
    }
}

/// One name to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What the name refers to.
    pub kind: CheckKind,
    /// The name as written in the config.
    pub name: String,
}

/// Outcome of checking one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Found. Only commands have a location, and only probed commands a version.
    Found {
        /// Resolved absolute path (commands only).
        path: Option<PathBuf>,
        /// First line of `--version` output, when it could be read.
        version: Option<String>,
    },
    /// Not found.
    Missing,
    /// Could not be checked, for the given reason. Never counts as missing.
    Skipped(String),
}

/// An entry together with its outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    /// What was checked.
    pub entry: Entry,
    /// What came out of it.
    pub status: Status,
}

/// Lists the entries to check in report order: commands, then aliases, then functions.
/// A name repeated within one kind is listed once.
pub fn plan(config: &Config) -> Vec<Entry> {
    let groups = [
        (CheckKind::Command, &config.commands),
        (CheckKind::Alias, &config.aliases),
        (CheckKind::Function, &config.functions),
    ];
    let mut entries = Vec::new();
    for (kind, names) in groups {
        let mut seen = std::collections::HashSet::new();
        for name in names {
            if seen.insert(name.as_str()) {
                entries.push(Entry {
                    kind,
                    name: name.clone(),
                });
            }
        }
    }
    entries
}

/// `PATH` and the working directory, read once and reused for every lookup.
#[derive(Clone, Debug)]
pub struct SearchEnv {
    /// The `PATH` value, if set.
    path: Option<OsString>,
    /// Current directory, needed by `which` for relative `PATH` entries.
    cwd: PathBuf,
}

impl SearchEnv {
    /// Reads `PATH` and the current directory from the process environment.
    pub fn from_process() -> Self {
        Self {
            path: std::env::var_os("PATH"),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Builds an environment from explicit values (used in tests).
    #[cfg(test)]
    pub fn new(path: Option<OsString>, cwd: PathBuf) -> Self {
        Self { path, cwd }
    }

    /// Resolves `name` to an absolute path. `which` honours `PATHEXT` and searches each `PATH`
    /// entry on its own.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        which::which_in(name, self.path.as_deref(), &self.cwd).ok()
    }
}

/// Looks `name` up on `PATH`. No process is started.
pub fn check_command(env: &SearchEnv, name: &str) -> Status {
    match env.find(name) {
        Some(path) => Status::Found {
            path: Some(path),
            version: None,
        },
        None => Status::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(commands: &[&str], aliases: &[&str], functions: &[&str]) -> Config {
        let own = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
        Config {
            commands: own(commands),
            aliases: own(aliases),
            functions: own(functions),
        }
    }

    #[test]
    fn plan_groups_by_kind_in_config_order() {
        let entries = plan(&config(&["b", "a"], &["x"], &["f"]));
        let names: Vec<_> = entries.iter().map(|e| (e.kind, e.name.as_str())).collect();
        assert_eq!(
            names,
            [
                (CheckKind::Command, "b"),
                (CheckKind::Command, "a"),
                (CheckKind::Alias, "x"),
                (CheckKind::Function, "f"),
            ]
        );
    }

    #[test]
    fn plan_deduplicates_within_a_kind_only() {
        let entries = plan(&config(&["a", "b", "a"], &["a", "a"], &[]));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn command_lookup_searches_each_path_entry() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("sample-tool");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path =
            std::env::join_paths([dir.path().join("nope"), dir.path().to_path_buf()]).unwrap();
        let env = SearchEnv::new(Some(path), dir.path().to_path_buf());
        assert!(matches!(
            check_command(&env, "sample-tool"),
            Status::Found {
                path: Some(_),
                version: None
            }
        ));
        assert_eq!(check_command(&env, "other-tool"), Status::Missing);
    }

    #[test]
    fn unset_path_finds_nothing() {
        let env = SearchEnv::new(None, PathBuf::from("."));
        assert_eq!(check_command(&env, "sh"), Status::Missing);
    }
}
