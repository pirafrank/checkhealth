[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml/badge.svg)](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml)

# 🤒 checkhealth

A cross-platform utility to check your toolbelt for necessary tools.

It reads a list of names from a YAML file and prints a pass/fail line for each one, with the
resolved location and version where available. It is read-only: it never installs, updates or
fixes anything.

```text
-------- Commands (1/2 found) --------
[✓] cargo  /home/me/.cargo/bin/cargo  (cargo 1.85.0)
[✗] rust

-------- Functions (0/0 found) --------
[-] paste-image  (skipped: not supported on cmd.exe)

1/2 found
1 skipped: paste-image
```

## Goals

- Check the tools in your toolbelt
- Tell you what is installed, which version, and where

## Non-goals

- Install or update tools
- Fix your configuration

## Install

### Install script (Linux, macOS, FreeBSD)

```sh
curl -fsSL https://raw.githubusercontent.com/pirafrank/checkhealth/main/install.sh | sh
```

It installs the latest release to `~/.local/bin` after verifying its checksum. Running it again
updates the binary. Set `CHECKHEALTH_TAG` to pick a release and `CHECKHEALTH_INSTALL_DIR` to
change the destination.

### Install script (Windows)

In PowerShell:

```powershell
irm https://raw.githubusercontent.com/pirafrank/checkhealth/main/install.ps1 | iex
```

This runs the script in memory, so the execution policy does not apply. The script is not
signed. If you save `install.ps1` first and your policy blocks it, either unblock the file
(`Unblock-File .\install.ps1`) or allow unsigned scripts for the current session only:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

It installs the latest release to `%LOCALAPPDATA%\Programs\checkhealth`, verifies its checksum
and adds the folder to your user `PATH`. It accepts the same `CHECKHEALTH_TAG` and
`CHECKHEALTH_INSTALL_DIR` variables.

### With poof (Linux, macOS)

Use [poof](https://poof.fpira.com) to install and keep it updated easily:

```sh
poof install pirafrank/checkhealth
```

### From the releases page

Download the latest binary for your platform from the
[releases](https://github.com/pirafrank/checkhealth/releases) page and put it in a folder in
your `$PATH`.

Each release ships a signed checksums manifest, an SBOM and a build provenance attestation.

## Supported platforms

- Linux (glibc and musl; x86_64, aarch64)
- macOS (x86_64, aarch64)
- FreeBSD (x86_64)
- Windows (MSVC x86_64/aarch64, GNU x86_64).

To build from source (Rust 1.85 or later): `cargo install --path .`

## Config file

```yaml
commands:   # executables found on PATH
  - cargo
  - git
aliases:    # shell aliases
  - ll
functions:  # shell functions
  - yy
```

All keys are optional and default to an empty list. Unknown keys are an error, so a typo such as
`command:` is not silently ignored. Names must be non-empty, must not start with `-`, and may
contain only letters, digits and `_ - . + @ : ,`: no whitespace or shell metacharacters. A name
listed twice within a kind is checked once. [`config.sample.yml`](config.sample.yml) is a working
example.

## Usage

```sh
checkhealth -c /path/to/config.yml
checkhealth -c /path/to/config.yml -s zsh
checkhealth completions --shell bash
```

| Option | Description |
|---|---|
| `-c, --config <FILE>` | Path to the YAML config file (required, except for `completions`) |
| `-s, --shell <SHELL>` | Shell used for aliases and functions: a name or a path (see *Shells*) |
| `--color <WHEN>` | `auto` (default: colour when stdout is a terminal), `always` or `never` |
| `-e, --export <FORMAT>` | `text`, `tsv`, `csv` or `json`. Default: `text` on a terminal, `tsv` when stdout is piped or redirected |
| `--no-version` | Do not run tools to read their version |
| `--timeout <SECONDS>` | Per-process timeout for version probes (default 2 s) and the shell (default 5 s) |
| `-v` … `-vvvv`, `-q` | Verbosity (default level: info) |
| `--help`, `--version` | Help, and long version information |

Results go to stdout, logs and errors to stderr. With `auto`, colour is used when stdout is a
terminal; `NO_COLOR` turns it off and `CLICOLOR_FORCE` turns it on. `--color` overrides both.
An interactive terminal, or `--color always` without `-e`, keeps the coloured text report.

## Supported shells

| Platform | Supported shells |
|---|---|
| Unix | `zsh`, `bash`, `sh` |
| Windows | `cmd.exe`, PowerShell 5 (`powershell.exe`), PowerShell 7+ (`pwsh.exe`) |

Without `--shell`, checkhealth uses `powershell.exe` on Windows; on Unix it uses `zsh` if
`$SHELL` contains "zsh", `bash` if `$SHELL` is set to anything else, and `sh` if it is unset.
An unsupported or non-existent shell is an error (exit code 4).

Commands are looked up on `PATH` in-process, without a shell. Aliases and functions need a
shell: checkhealth starts it **once** for all of them, as an interactive shell (`zsh -i`,
`bash -i`; PowerShell with its profile loaded), so that your rc files are read. Caveats:

- Detection depends on your rc files being loadable by a non-login interactive shell. An alias
  defined only in a login file, or behind an interactivity check that fails here, is reported
  as missing.
- `sh` has no portable rc loading, and `cmd.exe` macros (`doskey`) are invisible to a child
  process: aliases and functions are reported as `skipped`, not guessed.
- The shell gets 5 s (`--timeout`). On timeout all aliases and functions are `skipped
  (shell timed out)` and the run continues.
- Stderr noise from rc files is discarded unless you pass `-vv` or higher.

### Exporting for scripts

Piped output is machine-readable TSV by default, and `-e` picks `text`, `tsv`, `csv` or `json`.

See [docs/exporting.md](docs/exporting.md) for the formats, the fields and examples for zsh, bash
and PowerShell.

## How it works

checkhealth checks the three kinds of entries in different ways:

1. **Commands** are looked up on `PATH` inside checkhealth itself, with no shell involved. For
   each one found, it also runs `<tool> --version` to show the version (see *Versions*).
2. **Aliases and functions** only exist inside a shell, so checkhealth asks one. It starts the
   shell **once** for all names, as `<shell> -i -c '<fixed script>' ...`. The script tests each
   name and prints one marker line per name, and checkhealth reads only those lines, so anything
   your rc files print is ignored. Names are passed as arguments and are never interpolated into
   the script.
3. The shell runs concurrently with the command lookups and version probes. Results are
   collected and printed in config order, whatever order they finished in.

### Interactive shells that never draw a prompt

The `-i` flag makes the shell read your rc file (`~/.zshrc`, `~/.bashrc`), which is where aliases
and functions usually live. But `-i -c '<script>'` runs the script and exits **without ever
displaying a prompt**. It is an interactive shell in the sense that it loads your interactive
configuration, but it is not the same as the terminal you type into.

This matters if your rc files defer work until the first prompt, which is a common startup
optimisation. Examples are zsh `precmd` hooks and `PROMPT_COMMAND` in bash. Tools that are
lazy-loaded this way, such as `nvm`, `rvm` or `pyenv` set up by a one-shot prompt hook, are not
loaded when checkhealth asks about them. They can be reported as **missing** even though they
work fine in your terminal.

**This is not a bug in checkhealth.** The same thing happens to any `zsh -ic '...'` or
`bash -ic '...'` command, and the fix belongs in your shell configuration. For example, define a
small stub function that loads the real tool on first call, or load it from a file read by every
interactive shell instead of from a prompt hook. To see what checkhealth sees, run the shell the
same way:

```sh
zsh -ic 'whence -w nvm'    # zsh
bash -ic 'declare -F nvm'  # bash
```

If that prints nothing, checkhealth will report the entry as missing. Run with `-vv` to see the
stderr of the shell, which helps with debugging.

## Exit codes

| Code | Name | Meaning |
|---|---|---|
| 0 | `OK` | All checked entries found (skipped entries do not count as missing) |
| 1 | `MISSING` | The run completed and at least one entry is missing |
| 2 | `USAGE` | Invalid command line |
| 3 | `CONFIG` | Config file missing, unreadable, invalid YAML, unknown key or invalid name |
| 4 | `SHELL` | The requested or default shell is unsupported or cannot be started |
| 5 | `INTERNAL` | Any other unexpected error, including panics |

Codes 2 to 5 mean no report was produced: the message goes to stderr as `error: …` and no
summary line is printed. Per-entry problems (a probe timeout, a shell timeout, an unsupported
alias check) are shown as `skipped` in the report and never change the exit code. The numbers
are stable across releases.

## Versions: this runs your tools

For every command found, checkhealth runs `<tool> --version` (stdin closed, 2 s timeout, output
capped; on Unix a timed-out tool is killed together with its process group) and shows the first
non-empty line. **This executes the tools listed in the config.** Use `--no-version` to skip
it, in particular when the config comes from someone you do not trust. A failed probe only
omits the version; it never changes the result.

## Performance

The expensive part of a run is starting processes: one shell for all aliases and functions, and one
`<tool> --version` per command found. Everything else (reading the config, looking names up on
`PATH`, printing) takes microseconds. checkhealth is built so those processes overlap instead of
queueing, with plain threads from the standard library: no async runtime and no thread-pool crate.

```text
t0 ──► shell thread ────────────[ one `<shell> -i -c ...` for all aliases/functions ]──────┐
 │                                                                                         ├─► join ─► report
 └──► main thread: look up every command on PATH ─► probe pool (up to 8 workers) ──────────┘
```

- **The shell starts first.** It is the slowest part (it loads your rc files), so it is spawned on
  its own thread before anything else and runs while the rest of the work happens. If the config
  has no aliases or functions, no shell is started at all.
- **Lookups are cheap and sequential.** Commands are resolved in-process by walking `PATH`, one
  pass per name, with no process spawned. Duplicate names are checked once.
- **Probes run on a bounded pool.** The found commands are probed by `min(CPU cores, 8)` workers
  that pull the next job from a shared counter, so a slow tool never holds up the others and
  there is no queue or channel to manage. Cores beyond 8 add nothing: probes are short, and an
  unbounded pool would only add spawn pressure.
- **Order is deterministic.** Each result is stored at the index of its entry, so the report
  follows the config order whatever order things finished in.
- **The total is the slower path, not the sum.** Wall time is about `max(shell, lookups + probes)`
  plus a few milliseconds, not their sum. With 50 commands and 10 aliases, the shell time is
  hidden behind the probes, or the other way round.
- **Slow or misbehaving children are contained.** Every child gets no stdin, a timeout (2 s for
  probes, 5 s for the shell; see `--timeout`) and a cap on the output kept (4 KiB for probes). On
  timeout, or when a tool floods its output, the child is killed, together with its whole process
  group on Unix so version-manager shims do not leave orphans behind. A timed-out probe only
  loses the version of that tool. A timed-out shell turns its aliases and functions into
  `skipped`, not `missing`, and the run still completes. In the worst case, when every probe
  hangs, the time is bounded by `ceil(found commands / workers) × timeout`.
- **A panic never hangs the run.** A panic on any thread is reported and exits with code 5.

`--no-version` removes the probes entirely, which is the fastest mode and runs no tool from your
config.

## Benchmarks

These are the numbers I got on my machine, running checkhealth against my own
[dotfiles config](https://github.com/pirafrank/dotfiles/blob/d1d8de45e0a1226a8251c21c81424966b2bae407/checkhealth/config.yml): 139
entries (87 commands, 26 aliases, 26 functions) on my real `PATH`, with my real interactive zsh and
rc files. The machine is an 11th Gen Intel Core i7-11800H with 32GB RAM running Ubuntu 24.04 on
WSL2 (Linux 6.18, having 4 cores and 16GB RAM made available), with zsh 5.9, hyperfine 1.18.0 and
a 1281 KiB release binary, measured on 2026-11-23.

Each result is 30 runs after 5 warm-up runs. Treat them as orders of magnitude: your machine, rc
files and `PATH` will differ.

| Run | Mean ± σ | p95 |
|---|---|---|
| `checkhealth -c config.yml` | 1083 ± 57 ms | 1168 ms |
| `checkhealth -c config.yml --no-version` | 199 ± 7 ms | 210 ms |
| `zsh -ic exit` (the floor for the alias/function check) | 196 ± 12 ms | 209 ms |

What the numbers say:

- **Without versions, the shell is the whole cost.** `--no-version` takes as long as just starting
  my interactive zsh and exiting (about 200 ms, mostly my rc files). Looking up 87 commands on
  `PATH` is lost in that noise, and checking all 52 aliases and functions costs one shell start.
- **Versions cost about 0.9 s here, and a few tools set the pace.** Run one after another, the
  `--version` of the config names that resolve to a binary adds up to about 3.5 s. With the probe pool it is about
  0.9 s of extra wall time, a bit under the slowest single tool: `cs` alone takes about 1.2 s, then
  `code` about 450 ms and `serverless` about 400 ms. Adding cores would not help much; skipping
  those tools or using `--no-version` would.
- **Everyday use:** my whole toolbelt, 139 entries, is checked in about a second with versions,
  and in about 0.2 s without.

To measure your own config the same way (a non-zero exit code only means something is missing):

```sh
hyperfine -N -i --warmup 5 --runs 30 \
  'checkhealth -c config.yml' 'checkhealth -c config.yml --no-version' 'zsh -ic exit'
```

For more, [`bench/`](bench) has synthetic scenarios with a controlled `PATH` and pass/fail targets
(startup, 50 commands, aliases, overlap, hanging probes), driven by `bench/run.sh`; the targets are
in [`bench/targets.json`](bench/targets.json). Run them with
[hyperfine](https://github.com/sharkdp/hyperfine):

```sh
just bench-e2e      # run and print a summary
just bench-check    # fail if a target in bench/targets.json is missed
just bench-baseline # save bench/baseline.json
```

The [Bench workflow](.github/workflows/bench.yml) runs `just bench-check` on Ubuntu and uploads the
raw hyperfine JSON as an artifact. It is not part of pull request checks, since wall-clock numbers
on shared runners are noisy. It runs on demand and on release tags. See the workflow and
`bench/run.sh` for the full pipeline.

## Development

Many development tasks are automated using `just`, the task runner.

- `just` is the task runner (`just --list`)
- `just ci` runs the same checks as the CI workflow
- `just test` the tests (with the `test-hooks` feature, which enables hidden failure triggers
  used only by tests)
- `just man` generates the man page.

See [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines and contribution instructions.

## License

MIT
