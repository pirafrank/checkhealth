//! Rendering of results. Kept apart from checking so other formats can be added later.

use std::io::{self, Write};

use anstream::{AutoStream, ColorChoice};
use anstyle::{AnsiColor, Style};

use crate::check::{CheckResult, Status};
use crate::cli::ColorWhen;

/// Style of the `[✓]` marker.
const GREEN: Style = AnsiColor::Green.on_default();
/// Style of the `[✗]` marker.
const RED: Style = AnsiColor::Red.on_default();
/// Style of the `[-]` marker.
const YELLOW: Style = AnsiColor::Yellow.on_default();

/// Counts shown in the summary line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Entries found.
    pub found: usize,
    /// Entries missing.
    pub missing: usize,
    /// Names of the entries that were skipped.
    pub skipped: Vec<String>,
}

impl Summary {
    /// Tallies `results`.
    pub fn of(results: &[CheckResult]) -> Self {
        let mut summary = Self::default();
        for result in results {
            match &result.status {
                Status::Found { .. } => summary.found += 1,
                Status::Missing => summary.missing += 1,
                Status::Skipped(_) => summary.skipped.push(result.entry.name.clone()),
            }
        }
        summary
    }
}

/// Maps the `--color` flag to a stream colour choice. `auto` also honours `NO_COLOR`,
/// `CLICOLOR_FORCE` and terminal detection (done by `anstream`).
pub fn color_choice(when: ColorWhen) -> ColorChoice {
    match when {
        ColorWhen::Auto => ColorChoice::Auto,
        ColorWhen::Always => ColorChoice::Always,
        ColorWhen::Never => ColorChoice::Never,
    }
}

/// Writes one line per result, then a blank line and the summary.
///
/// Styling is always emitted; wrap the writer in an [`AutoStream`] to strip it when colour is
/// off.
pub fn render(results: &[CheckResult], out: &mut dyn Write) -> io::Result<()> {
    for result in results {
        render_line(result, out)?;
    }
    let summary = Summary::of(results);
    writeln!(out)?;
    writeln!(
        out,
        "{}/{} found",
        summary.found,
        summary.found + summary.missing
    )?;
    if !summary.skipped.is_empty() {
        writeln!(
            out,
            "{} skipped: {}",
            summary.skipped.len(),
            summary.skipped.join(", ")
        )?;
    }
    Ok(())
}

/// Writes the line for one result.
fn render_line(result: &CheckResult, out: &mut dyn Write) -> io::Result<()> {
    let name = &result.entry.name;
    match &result.status {
        Status::Found { path, version } => {
            write!(out, "{GREEN}[✓]{GREEN:#} {name}")?;
            if let Some(path) = path {
                write!(out, "  {}", path.display())?;
            }
            if let Some(version) = version {
                write!(out, "  ({version})")?;
            }
        }
        Status::Missing => write!(out, "{RED}[✗]{RED:#} {name}")?,
        Status::Skipped(reason) => {
            write!(out, "{YELLOW}[-]{YELLOW:#} {name}  (skipped: {reason})")?
        }
    }
    writeln!(out)
}

/// Prints the report to stdout, coloured according to `when`.
pub fn print(results: &[CheckResult], when: ColorWhen) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = AutoStream::new(stdout.lock(), color_choice(when));
    render(results, &mut out)?;
    out.flush()
}

/// Prints `error: <message>` to stderr. The word `error` is coloured by the same rule as the
/// report, applied to stderr.
pub fn print_error(message: &str, when: ColorWhen) {
    let stderr = io::stderr();
    let mut err = AutoStream::new(stderr.lock(), color_choice(when));
    // Nothing sensible can be done if stderr itself is broken.
    let _ = writeln!(err, "{RED}error{RED:#}: {message}");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::check::{CheckKind, Entry};

    fn result(name: &str, status: Status) -> CheckResult {
        CheckResult {
            entry: Entry {
                kind: CheckKind::Command,
                name: name.into(),
            },
            status,
        }
    }

    fn sample() -> Vec<CheckResult> {
        vec![
            result(
                "cargo",
                Status::Found {
                    path: Some(PathBuf::from("/bin/cargo")),
                    version: Some("cargo 1.85.0".into()),
                },
            ),
            result("rust", Status::Missing),
            result(
                "paste-image",
                Status::Skipped("not supported on cmd.exe".into()),
            ),
        ]
    }

    fn plain(results: &[CheckResult]) -> String {
        let mut out = AutoStream::never(Vec::new());
        render(results, &mut out).unwrap();
        String::from_utf8(out.into_inner()).unwrap()
    }

    #[test]
    fn plain_output_matches_the_spec() {
        assert_eq!(
            plain(&sample()),
            "[✓] cargo  /bin/cargo  (cargo 1.85.0)\n\
             [✗] rust\n\
             [-] paste-image  (skipped: not supported on cmd.exe)\n\
             \n\
             1/2 found\n\
             1 skipped: paste-image\n"
        );
    }

    #[test]
    fn colour_is_emitted_when_asked_for() {
        let mut out = AutoStream::always(Vec::new());
        render(&sample(), &mut out).unwrap();
        let text = String::from_utf8(out.into_inner()).unwrap();
        assert!(text.contains("\x1b[32m[✓]"), "{text:?}");
        assert!(text.contains("\x1b[31m[✗]"), "{text:?}");
        assert!(text.contains("\x1b[33m[-]"), "{text:?}");
    }

    #[test]
    fn summary_excludes_skipped_entries() {
        let summary = Summary::of(&sample());
        assert_eq!(
            summary,
            Summary {
                found: 1,
                missing: 1,
                skipped: vec!["paste-image".to_string()]
            }
        );
    }

    #[test]
    fn empty_report_still_has_a_summary() {
        assert_eq!(plain(&[]), "\n0/0 found\n");
    }
}
