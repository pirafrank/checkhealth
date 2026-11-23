[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml/badge.svg)](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml)

# checkhealth

A cross-platform utility to check your toolbelt for necessary tools.

It reads a list of names from a YAML file and prints a pass/fail line for each one, with the
resolved location and version where available. It is read-only: it never installs, updates or
fixes anything.

```text
[✓] cargo  /home/me/.cargo/bin/cargo  (cargo 1.85.0)
[✗] rust
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
| `--no-version` | Do not run tools to read their version |
| `--timeout <SECONDS>` | Per-process timeout for version probes (default 2 s) and the shell (default 5 s) |
| `-v` … `-vvvv`, `-q` | Verbosity (default level: info) |
| `--help`, `--version` | Help, and long version information |

Results go to stdout, logs and errors to stderr. With `auto`, colour is used when stdout is a
terminal; `NO_COLOR` turns it off and `CLICOLOR_FORCE` turns it on. `--color` overrides both.

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

## Versions: this runs your tools

For every command found, checkhealth runs `<tool> --version` (stdin closed, 2 s timeout, output
capped; on Unix a timed-out tool is killed together with its process group) and shows the first
non-empty line. **This executes the tools listed in the config.** Use `--no-version` to skip
it, in particular when the config comes from someone you do not trust. A failed probe only
omits the version; it never changes the result.

## Performance

Work runs concurrently: the shell starts first on its own thread while commands are looked up
and probed by a pool of at most 8 threads, so the total time is about the slower of the two.
Without aliases or functions no shell is started at all. Typical numbers (see
[`bench/`](bench)): `--version` in about 1 ms, 50 commands without versions in about 2 ms, with
versions in about 20 ms.

Run the end-to-end benchmarks with [hyperfine](https://github.com/sharkdp/hyperfine):

```sh
just bench-e2e      # run and print a summary
just bench-check    # fail if a target in bench/targets.json is missed
just bench-baseline # save bench/baseline.json
```

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
