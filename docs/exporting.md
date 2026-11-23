# Exporting for scripts

The report format depends on where stdout goes:

| stdout | Default format |
|---|---|
| A terminal | `text`: the coloured report shown in the [README](../README.md) |
| A pipe or a file | `tsv`: one machine-readable line per entry, no colour |

`-e, --export <text|tsv|csv|json>` overrides this in both directions. `--color always` without
`-e` also keeps the text report, so `checkhealth -c cfg.yml --color always | less -R` still works.

**TSV** and **CSV** have no header and one line per entry, in report order (commands, aliases,
functions). The columns are `kind`, `name`, `status`, `path`, `version` and `reason`; cells that
do not apply are empty. In TSV, tabs, newlines and backslashes inside values are escaped as `\t`,
`\n` and `\\`. In CSV, cells containing a comma, a double quote or a line break are quoted
(RFC 4180) and quotes are doubled. Lines end with `\n`, not `\r\n`.

```text
command	cargo	found	/home/me/.cargo/bin/cargo	cargo 1.85.0	
command	rust	missing			
function	paste-image	skipped			not supported on cmd.exe
```

**JSON** is a single array with the same six fields per object, using `null` where a field does
not apply:

```json
[{"kind":"command","name":"cargo","status":"found","path":"/home/me/.cargo/bin/cargo","version":"cargo 1.85.0","reason":null}]
```

Allowed values and when each field is set:

| Field | Values / content |
|---|---|
| `kind` | `command`, `alias` or `function` |
| `status` | `found`, `missing` or `skipped` |
| `path` | Resolved location. Only for `found` commands |
| `version` | First line of `<tool> --version`. Only for `found` entries that were probed (not with `--no-version`) |
| `reason` | Why the entry was not checked. Only for `skipped` |

Examples:

```sh
checkhealth -c cfg.yml | awk -F'\t' '$3 == "missing" { print $2 }'   # names of missing tools
checkhealth -c cfg.yml -e json | jq -r '.[] | select(.status=="missing") | .name'
checkhealth -c cfg.yml -e text > report.txt                          # human report in a file
```

## Per-shell examples

**zsh (Linux, macOS)**: collect the missing names into an array and split a line into fields. Use
`(ps:\t:)`, not `(s:\t:)`, so the tab escape is understood, and do not name a variable `status`
(it is read-only in zsh).

```zsh
missing=(${(f)"$(checkhealth -c cfg.yml | awk -F'\t' '$3 == "missing" { print $2 }')"})
(( $#missing )) && print -l -- "missing:" $missing

checkhealth -c cfg.yml | while IFS= read -r line; do
  fields=("${(@ps:\t:)line}")      # kind name status path version reason
  print "$fields[2] -> $fields[3]"
done
```

**bash (FreeBSD)**: bash is not in the base system (`pkg install bash`), so it lives in
`/usr/local/bin`. The base `awk` and `cut` work with the TSV as is. `read` with a tab `IFS`
collapses empty cells, so read only the leading columns, which are never empty, and keep the rest
in one variable.

```bash
mapfile -t missing < <(checkhealth -c cfg.yml | awk -F'\t' '$3 == "missing" { print $2 }')
printf 'missing: %s\n' "${missing[@]}"

checkhealth -c cfg.yml | while IFS=$'\t' read -r kind name state rest; do
  echo "$name -> $state"
done

checkhealth -c cfg.yml | cut -f2,3                 # name and status, empty cells preserved
```

**pwsh (Windows)**: CSV or JSON turn straight into objects, so no text parsing is needed. TSV has
no header, so give the column names to `ConvertFrom-Csv`. PowerShell 7 reads native output as UTF-8
by default. In Windows PowerShell 5.1 set `[Console]::OutputEncoding = [Text.Encoding]::UTF8` first.

```powershell
checkhealth.exe -c cfg.yml -e csv |
  ConvertFrom-Csv -Header kind,name,status,path,version,reason |
  Where-Object status -eq missing |
  ForEach-Object name

checkhealth.exe -c cfg.yml -e json | ConvertFrom-Json | Where-Object status -ne found

checkhealth.exe -c cfg.yml -e text > report.txt   # human report in a file
if ($LASTEXITCODE -eq 1) { Write-Warning "some tools are missing" }
```

The exit codes are the same for every format, and the output is identical on all platforms
(UTF-8). On Windows PowerShell 5.1, piping to another program re-encodes text; use `pwsh`, or set
`[Console]::OutputEncoding` to UTF-8 if names are not ASCII.
