//! Rendering of results. Kept apart from checking so other formats can be added later.

use std::io::{self, Write};

use anstream::{AutoStream, ColorChoice};
use anstyle::{AnsiColor, Style};
use serde::Serialize;

use crate::check::{CheckKind, CheckResult, Status};
use crate::cli::{ColorWhen, ExportFormat};

/// Style of the `[✓]` marker.
const GREEN: Style = AnsiColor::Green.on_default();
/// Style of the `[✗]` marker.
const RED: Style = AnsiColor::Red.on_default();
/// Style of the `[-]` marker.
const YELLOW: Style = AnsiColor::Yellow.on_default();

/// Dashes placed before and after each group heading.
const RULE: &str = "--------";

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
    pub fn of<'a>(results: impl IntoIterator<Item = &'a CheckResult>) -> Self {
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

/// Writes the results grouped by kind (commands, aliases, functions), each group under a heading
/// with its own count, then a blank line and the overall summary. Groups without entries are
/// omitted.
///
/// Styling is always emitted; wrap the writer in an [`AutoStream`] to strip it when colour is
/// off.
pub fn render(results: &[CheckResult], out: &mut dyn Write) -> io::Result<()> {
    let mut first_group = true;
    for kind in CheckKind::ALL {
        let group: Vec<&CheckResult> = results.iter().filter(|r| r.entry.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        if !first_group {
            writeln!(out)?;
        }
        first_group = false;

        let counts = Summary::of(group.iter().copied());
        writeln!(
            out,
            "{RULE} {} ({}/{} found) {RULE}",
            kind.heading(),
            counts.found,
            counts.found + counts.missing
        )?;
        for result in group {
            render_line(result, out)?;
        }
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

/// Resolves `--color` for a report that is rendered into a buffer and written later.
///
/// `auto` is decided from stdout (terminal, `NO_COLOR`, `CLICOLOR_FORCE`), because that is where
/// the report ends up, directly or through the pager.
pub fn report_color_choice(when: ColorWhen) -> ColorChoice {
    match when {
        ColorWhen::Auto => AutoStream::choice(&io::stdout()),
        ColorWhen::Always => ColorChoice::AlwaysAnsi,
        ColorWhen::Never => ColorChoice::Never,
    }
}

/// Renders the whole report in the given format into memory. `Text` is coloured according to
/// `color`; the export formats never contain colour.
pub fn render_as(
    results: &[CheckResult],
    format: ExportFormat,
    color: ColorChoice,
) -> io::Result<Vec<u8>> {
    match format {
        ExportFormat::Text => {
            let mut out = AutoStream::new(Vec::new(), color);
            render(results, &mut out)?;
            out.flush()?;
            Ok(out.into_inner())
        }
        ExportFormat::Tsv => to_bytes(|out| render_tsv(results, out)),
        ExportFormat::Csv => to_bytes(|out| render_csv(results, out)),
        ExportFormat::Json => to_bytes(|out| render_json(results, out)),
    }
}

/// Runs `render` against an in-memory buffer and returns what it wrote.
fn to_bytes(render: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    render(&mut buffer)?;
    Ok(buffer)
}

/// One entry of the export formats.
#[derive(Debug, Serialize)]
struct ExportRow<'a> {
    kind: &'static str,
    name: &'a str,
    status: &'static str,
    path: Option<String>,
    version: Option<&'a str>,
    reason: Option<&'a str>,
}

impl<'a> ExportRow<'a> {
    fn of(result: &'a CheckResult) -> Self {
        let (status, path, version, reason) = match &result.status {
            Status::Found { path, version } => (
                "found",
                path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                version.as_deref(),
                None,
            ),
            Status::Missing => ("missing", None, None, None),
            Status::Skipped(reason) => ("skipped", None, None, Some(reason.as_str())),
        };
        Self {
            kind: result.entry.kind.export_name(),
            name: &result.entry.name,
            status,
            path,
            version,
            reason,
        }
    }
}

impl ExportRow<'_> {
    /// The columns of the tabular formats; empty means "not applicable".
    fn cells(&self) -> [&str; 6] {
        [
            self.kind,
            self.name,
            self.status,
            self.path.as_deref().unwrap_or(""),
            self.version.unwrap_or(""),
            self.reason.unwrap_or(""),
        ]
    }
}

/// Escapes a TSV cell so a record always stays on one line with fixed columns.
fn escape_tsv(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Writes one tab-separated line per result: `kind name status path version reason`, in the same
/// order as the text report. There is no header and empty cells mean "not applicable". Lines end
/// with `\n` on every platform.
pub fn render_tsv(results: &[CheckResult], out: &mut dyn Write) -> io::Result<()> {
    for result in ordered(results) {
        let row = ExportRow::of(result);
        let cells = row.cells();
        let line: Vec<String> = cells.iter().map(|cell| escape_tsv(cell)).collect();
        writeln!(out, "{}", line.join("\t"))?;
    }
    Ok(())
}

/// Quotes a CSV cell (RFC 4180) when it contains a comma, a double quote or a line break;
/// embedded quotes are doubled.
fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Writes one comma-separated line per result, with the same columns and order as
/// [`render_tsv`]. There is no header. Lines end with `\n` (not `\r\n`) on every platform.
pub fn render_csv(results: &[CheckResult], out: &mut dyn Write) -> io::Result<()> {
    for result in ordered(results) {
        let row = ExportRow::of(result);
        let cells = row.cells();
        let line: Vec<String> = cells.iter().map(|cell| escape_csv(cell)).collect();
        writeln!(out, "{}", line.join(","))?;
    }
    Ok(())
}

/// Writes the results as one JSON array, in the same order as the text report.
pub fn render_json(results: &[CheckResult], out: &mut dyn Write) -> io::Result<()> {
    let rows: Vec<ExportRow> = ordered(results).map(ExportRow::of).collect();
    serde_json::to_writer(&mut *out, &rows).map_err(io::Error::from)?;
    out.write_all(b"\n")
}

/// Results grouped by kind (commands, aliases, functions), keeping the order within a group.
fn ordered(results: &[CheckResult]) -> impl Iterator<Item = &CheckResult> {
    CheckKind::ALL
        .into_iter()
        .flat_map(move |kind| results.iter().filter(move |r| r.entry.kind == kind))
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
        result_of(CheckKind::Command, name, status)
    }

    fn result_of(kind: CheckKind, name: &str, status: Status) -> CheckResult {
        CheckResult {
            entry: Entry {
                kind,
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
            result_of(
                CheckKind::Function,
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
            "-------- Commands (1/2 found) --------\n\
             [✓] cargo  /bin/cargo  (cargo 1.85.0)\n\
             [✗] rust\n\
             \n\
             -------- Functions (0/0 found) --------\n\
             [-] paste-image  (skipped: not supported on cmd.exe)\n\
             \n\
             1/2 found\n\
             1 skipped: paste-image\n"
        );
    }

    #[test]
    fn groups_follow_command_alias_function_order_and_skip_empty_ones() {
        let results = vec![
            result_of(CheckKind::Function, "yy", Status::Missing),
            result_of(CheckKind::Alias, "ll", Status::Missing),
            result_of(CheckKind::Alias, "l", Status::Missing),
        ];
        assert_eq!(
            plain(&results),
            "-------- Aliases (0/2 found) --------\n\
             [✗] ll\n\
             [✗] l\n\
             \n\
             -------- Functions (0/1 found) --------\n\
             [✗] yy\n\
             \n\
             0/3 found\n"
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

    fn tsv(results: &[CheckResult]) -> String {
        let mut out = Vec::new();
        render_tsv(results, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn tsv_has_one_line_per_entry_with_six_columns() {
        assert_eq!(
            tsv(&sample()),
            "command\tcargo\tfound\t/bin/cargo\tcargo 1.85.0\t\n\
             command\trust\tmissing\t\t\t\n\
             function\tpaste-image\tskipped\t\t\tnot supported on cmd.exe\n"
        );
    }

    #[test]
    fn tsv_escapes_separators_and_follows_group_order() {
        let results = vec![
            result_of(CheckKind::Function, "f", Status::Missing),
            result(
                "a",
                Status::Found {
                    path: None,
                    version: Some("v1\tx\ny\\z".into()),
                },
            ),
        ];
        assert_eq!(
            tsv(&results),
            "command\ta\tfound\t\tv1\\tx\\ny\\\\z\t\nfunction\tf\tmissing\t\t\t\n"
        );
    }

    #[test]
    fn tsv_of_nothing_is_empty() {
        assert_eq!(tsv(&[]), "");
    }

    fn csv(results: &[CheckResult]) -> String {
        let mut out = Vec::new();
        render_csv(results, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn csv_has_the_same_columns_as_tsv() {
        assert_eq!(
            csv(&sample()),
            "command,cargo,found,/bin/cargo,cargo 1.85.0,\n\
             command,rust,missing,,,\n\
             function,paste-image,skipped,,,not supported on cmd.exe\n"
        );
    }

    #[test]
    fn csv_quotes_commas_quotes_and_line_breaks() {
        let results = vec![result(
            "a",
            Status::Found {
                path: None,
                version: Some("v1, \"beta\"\nline2".into()),
            },
        )];
        assert_eq!(
            csv(&results),
            "command,a,found,,\"v1, \"\"beta\"\"\nline2\",\n"
        );
    }

    #[test]
    fn json_is_an_array_of_objects() {
        let mut out = Vec::new();
        render_json(&sample(), &mut out).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 3);
        assert_eq!(value[0]["kind"], "command");
        assert_eq!(value[0]["status"], "found");
        assert_eq!(value[0]["path"], "/bin/cargo");
        assert_eq!(value[1]["status"], "missing");
        assert!(value[1]["path"].is_null());
        assert_eq!(value[2]["reason"], "not supported on cmd.exe");
        assert!(out.ends_with(b"\n"));
    }

    #[test]
    fn empty_report_still_has_a_summary() {
        assert_eq!(plain(&[]), "\n0/0 found\n");
    }
}
