//! Concurrent scheduling of the checks.
//!
//! The expensive operations are process spawns: the one shell invocation for aliases and
//! functions, and one `--version` probe per found command. The shell invocation starts first on
//! its own thread; meanwhile the commands are looked up (microseconds) and probed by a small
//! bounded pool. Results are stored by index, so the report keeps the config order whatever the
//! completion order.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::Result;

use crate::check::{CheckKind, CheckResult, Entry, SearchEnv, Status, check_command};

/// Most probe threads ever started.
const MAX_THREADS: usize = 8;

/// Answers alias and function questions with a single shell invocation.
pub trait ShellChecker: Sync {
    /// Checks all `entries` (aliases and functions) and returns one status per entry, in the
    /// same order. Failures that affect the whole batch (timeout, unsupported shell) are
    /// reported as `Skipped` for every entry. An error means the shell could not be used at
    /// all and ends the run.
    fn check(&self, entries: &[Entry]) -> Result<Vec<Status>>;
}

/// Reads the version of the tool at a path. `None` when it cannot be determined.
pub type Probe<'a> = &'a (dyn Fn(&Path) -> Option<String> + Sync);

/// Number of probe threads: the available parallelism, capped.
pub fn default_threads() -> usize {
    thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS)
}

/// Runs all checks of a config.
pub struct Runner<'a> {
    /// Where commands are looked up.
    pub search: &'a SearchEnv,
    /// Version probe, or `None` to run no tools at all.
    pub probe: Option<Probe<'a>>,
    /// Handles aliases and functions.
    pub shell: &'a dyn ShellChecker,
    /// Size of the probe pool.
    pub threads: usize,
}

impl Runner<'_> {
    /// Checks every entry and returns the results in the order of `entries`.
    pub fn run(&self, entries: &[Entry]) -> Result<Vec<CheckResult>> {
        let shell_entries: Vec<Entry> = entries
            .iter()
            .filter(|e| e.kind != CheckKind::Command)
            .cloned()
            .collect();
        let mut statuses: Vec<Option<Status>> = vec![None; entries.len()];
        let mut shell_result: Result<Vec<Status>> = Ok(Vec::new());

        thread::scope(|scope| {
            // Start the slowest part first so it overlaps with everything else.
            let shell_job = (!shell_entries.is_empty())
                .then(|| scope.spawn(|| self.shell.check(&shell_entries)));

            let mut to_probe: Vec<(usize, PathBuf)> = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                if entry.kind != CheckKind::Command {
                    continue;
                }
                let status = check_command(self.search, &entry.name);
                if let (
                    Status::Found {
                        path: Some(path), ..
                    },
                    Some(_),
                ) = (&status, self.probe)
                {
                    to_probe.push((index, path.clone()));
                }
                statuses[index] = Some(status);
            }

            if let Some(probe) = self.probe {
                for (index, version) in self.probe_all(&to_probe, probe) {
                    if let Some(Status::Found { version: slot, .. }) = &mut statuses[index] {
                        *slot = version;
                    }
                }
            }

            if let Some(job) = shell_job {
                shell_result = job.join().expect("shell thread panicked");
            }
        });

        let mut shell_statuses = shell_result?.into_iter();
        let results = entries
            .iter()
            .zip(statuses)
            .map(|(entry, status)| {
                let status = status.unwrap_or_else(|| {
                    shell_statuses.next().unwrap_or_else(|| {
                        Status::Skipped("the shell returned no answer".to_string())
                    })
                });
                CheckResult {
                    entry: entry.clone(),
                    status,
                }
            })
            .collect();
        Ok(results)
    }

    /// Probes `jobs` (entry index and path) on a bounded pool; returns `(index, version)`.
    fn probe_all(
        &self,
        jobs: &[(usize, PathBuf)],
        probe: Probe<'_>,
    ) -> Vec<(usize, Option<String>)> {
        let workers = self.threads.clamp(1, MAX_THREADS).min(jobs.len());
        let next = AtomicUsize::new(0);
        let done = Mutex::new(Vec::with_capacity(jobs.len()));
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    #[cfg(feature = "test-hooks")]
                    if std::env::var("CHECKHEALTH_TEST_PANIC").is_ok_and(|v| v == "thread") {
                        panic!("induced on a worker thread");
                    }
                    loop {
                        let job = next.fetch_add(1, Ordering::Relaxed);
                        let Some((index, path)) = jobs.get(job) else {
                            break;
                        };
                        let version = probe(path);
                        done.lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push((*index, version));
                    }
                });
            }
        });
        done.into_inner().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    /// A shell that answers after a delay: aliases named `ok*` exist, others do not.
    struct SlowShell(Duration);

    impl ShellChecker for SlowShell {
        fn check(&self, entries: &[Entry]) -> Result<Vec<Status>> {
            thread::sleep(self.0);
            Ok(entries
                .iter()
                .map(|e| {
                    if e.name.starts_with("ok") {
                        Status::Found {
                            path: None,
                            version: None,
                        }
                    } else {
                        Status::Missing
                    }
                })
                .collect())
        }
    }

    fn entry(kind: CheckKind, name: &str) -> Entry {
        Entry {
            kind,
            name: name.to_string(),
        }
    }

    /// `PATH` made of a temp dir with empty executable files named `names`.
    fn search_env(names: &[&str]) -> (tempfile::TempDir, SearchEnv) {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            let path = dir.path().join(name);
            std::fs::write(&path, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let env = SearchEnv::new(Some(dir.path().into()), dir.path().to_path_buf());
        (dir, env)
    }

    fn names(results: &[CheckResult]) -> Vec<&str> {
        results.iter().map(|r| r.entry.name.as_str()).collect()
    }

    #[test]
    fn results_keep_the_input_order_whatever_the_completion_order() {
        let (_dir, search) = search_env(&["a", "b", "c", "d"]);
        // Earlier entries take longer, so they finish last.
        let probe = |path: &Path| {
            let name = path.file_name().unwrap().to_str().unwrap();
            let delay = 40 * (4 - u64::from(name.as_bytes()[0] - b'a'));
            thread::sleep(Duration::from_millis(delay));
            Some(format!("v-{name}"))
        };
        let shell = SlowShell(Duration::from_millis(1));
        let entries = [
            entry(CheckKind::Command, "a"),
            entry(CheckKind::Function, "ok-f"),
            entry(CheckKind::Command, "b"),
            entry(CheckKind::Command, "missing"),
            entry(CheckKind::Alias, "nope"),
            entry(CheckKind::Command, "c"),
            entry(CheckKind::Command, "d"),
        ];
        let runner = Runner {
            search: &search,
            probe: Some(&probe),
            shell: &shell,
            threads: 4,
        };
        let concurrent = runner.run(&entries).unwrap();
        let sequential = Runner {
            threads: 1,
            ..runner
        }
        .run(&entries)
        .unwrap();
        assert_eq!(concurrent, sequential);
        assert_eq!(names(&concurrent), names(&sequential));
        assert_eq!(
            names(&concurrent),
            ["a", "ok-f", "b", "missing", "nope", "c", "d"]
        );
        assert_eq!(
            concurrent[0].status,
            Status::Found {
                path: Some(search.find("a").unwrap()),
                version: Some("v-a".into())
            }
        );
        assert_eq!(concurrent[3].status, Status::Missing);
        assert_eq!(concurrent[4].status, Status::Missing);
        assert!(matches!(
            concurrent[1].status,
            Status::Found { path: None, .. }
        ));
    }

    #[test]
    fn slow_probes_run_in_parallel() {
        let (_dir, search) = search_env(&["a", "b", "c", "d", "e", "f"]);
        let probe = |_: &Path| {
            thread::sleep(Duration::from_millis(300));
            None
        };
        let entries: Vec<_> = ["a", "b", "c", "d", "e", "f"]
            .map(|n| entry(CheckKind::Command, n))
            .into();
        let shell = SlowShell(Duration::ZERO);
        let runner = Runner {
            search: &search,
            probe: Some(&probe),
            shell: &shell,
            threads: 6,
        };
        let started = Instant::now();
        let results = runner.run(&entries).unwrap();
        // Sequentially this would take 1.8 s.
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(results.len(), 6);
    }

    #[test]
    fn pool_size_bounds_the_parallelism() {
        let (_dir, search) = search_env(&["a", "b", "c", "d"]);
        let running = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let probe = |_: &Path| {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(50));
            running.fetch_sub(1, Ordering::SeqCst);
            None
        };
        let entries: Vec<_> = ["a", "b", "c", "d"]
            .map(|n| entry(CheckKind::Command, n))
            .into();
        let shell = SlowShell(Duration::ZERO);
        Runner {
            search: &search,
            probe: Some(&probe),
            shell: &shell,
            threads: 2,
        }
        .run(&entries)
        .unwrap();
        assert!(peak.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn shell_and_probes_overlap() {
        let (_dir, search) = search_env(&["a", "b"]);
        let probe = |_: &Path| {
            thread::sleep(Duration::from_millis(300));
            None
        };
        let shell = SlowShell(Duration::from_millis(300));
        let entries = [
            entry(CheckKind::Alias, "ok"),
            entry(CheckKind::Command, "a"),
            entry(CheckKind::Command, "b"),
        ];
        let runner = Runner {
            search: &search,
            probe: Some(&probe),
            shell: &shell,
            threads: 2,
        };
        let started = Instant::now();
        runner.run(&entries).unwrap();
        // 300 ms shell + 300 ms probes would be 600 ms if run one after the other.
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn no_probe_means_no_versions_and_no_tool_runs() {
        let (_dir, search) = search_env(&["a"]);
        let shell = SlowShell(Duration::ZERO);
        let runner = Runner {
            search: &search,
            probe: None,
            shell: &shell,
            threads: 2,
        };
        let results = runner.run(&[entry(CheckKind::Command, "a")]).unwrap();
        assert!(matches!(
            results[0].status,
            Status::Found { version: None, .. }
        ));
    }

    #[test]
    fn shell_is_not_used_without_aliases_or_functions() {
        struct NeverShell;
        impl ShellChecker for NeverShell {
            fn check(&self, _: &[Entry]) -> Result<Vec<Status>> {
                panic!("the shell must not be started");
            }
        }
        let (_dir, search) = search_env(&["a"]);
        let runner = Runner {
            search: &search,
            probe: None,
            shell: &NeverShell,
            threads: 2,
        };
        runner.run(&[entry(CheckKind::Command, "a")]).unwrap();
    }

    #[test]
    fn a_short_shell_answer_degrades_to_skipped() {
        struct Lazy;
        impl ShellChecker for Lazy {
            fn check(&self, _: &[Entry]) -> Result<Vec<Status>> {
                Ok(Vec::new())
            }
        }
        let (_dir, search) = search_env(&[]);
        let runner = Runner {
            search: &search,
            probe: None,
            shell: &Lazy,
            threads: 1,
        };
        let results = runner.run(&[entry(CheckKind::Alias, "x")]).unwrap();
        assert!(matches!(results[0].status, Status::Skipped(_)));
    }
}
