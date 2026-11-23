#!/bin/sh
# End-to-end timing benchmarks for checkhealth, using hyperfine.
#
#   bench/run.sh                  run all scenarios and print a summary
#   bench/run.sh --check          also fail if a target in bench/targets.json is missed
#                                 (and if a baseline exists, if anything is >25% slower)
#   bench/run.sh --export FILE    also save the results as a baseline
#   bench/run.sh --runs N         runs per scenario (default 30); --warmup N (default 5)
#
# Needs the release binary (`just bench-build`) and hyperfine (not installed automatically).
# PATH and HOME are controlled: PATH holds only symlinks to a few tools and the fixtures in
# bench/fixtures, HOME holds the minimal rc files in bench/rc. One extra scenario uses the
# real PATH.
set -eu
unset CDPATH

root=$(cd "$(dirname "$0")/.." && pwd)
bench=$root/bench
bin=$root/target/release/checkhealth
runs=30
warmup=5
check=0
export_file=
baseline=$bench/baseline.json

while [ $# -gt 0 ]; do
    case $1 in
        --check) check=1 ;;
        --export) export_file=$2; shift ;;
        --runs) runs=$2; shift ;;
        --warmup) warmup=$2; shift ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "bench/run.sh: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

hyperfine=$(command -v hyperfine || true)
if [ -z "$hyperfine" ]; then
    echo "bench/run.sh: hyperfine is required (https://github.com/sharkdp/hyperfine); install it and retry." >&2
    exit 127
fi
if [ ! -x "$bin" ]; then
    echo "bench/run.sh: $bin not found; run 'just bench-build' first." >&2
    exit 1
fi
if [ "$runs" -le 1 ] && [ "$warmup" -eq 5 ]; then
    warmup=0
fi

real_path=$PATH
strace=$(command -v strace || true)
python=$(command -v python3 || true)
uname_s=$(uname -s)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
out=$root/target/bench
rm -rf "$out"
mkdir -p "$out" "$work/bin" "$work/home" "$work/home-slow"

# Controlled PATH: a few real tools (the fixtures and the shells need them) plus fakes.
for tool in sh bash zsh sleep ls cat grep sed head; do
    found=$(command -v "$tool" || true)
    case $found in /*) ln -s "$found" "$work/bin/$tool" ;; esac
done
i=1
while [ "$i" -le 40 ]; do
    ln -s "$bench/fixtures/fast-tool" "$(printf '%s/bin/tool-%02d' "$work" "$i")"
    i=$((i + 1))
done
for n in 1 2 3 4; do ln -s "$bench/fixtures/medium-tool" "$work/bin/medium-tool-$n"; done
ln -s "$bench/fixtures/sleep-tool" "$work/bin/sleep-tool"

cp "$bench/rc/.bashrc" "$bench/rc/.zshrc" "$work/home/"
cp "$bench/fixtures/slow-rc/.bashrc" "$bench/fixtures/slow-rc/.zshrc" "$work/home-slow/"

export PATH=$work/bin
export HOME=$work/home
unset ZDOTDIR NO_COLOR CLICOLOR_FORCE || true

# Every config must run to completion (exit 0 or 1) before it is timed.
validate() {
    status=0
    "$bin" "$@" >/dev/null 2>"$work/stderr" || status=$?
    if [ "$status" -gt 1 ]; then
        echo "bench/run.sh: 'checkhealth $*' exited with $status:" >&2
        cat "$work/stderr" >&2
        exit 1
    fi
}

# scenario NAME HOME COMMAND... : time one scenario. -i because missing tools exit with 1.
scenario() {
    name=$1
    home=$2
    shift 2
    echo "== $name" >&2
    HOME=$home "$hyperfine" --warmup "$warmup" --runs "$runs" --shell=none -i \
        --command-name "$name" --export-json "$out/$name.json" "$@" >/dev/null
}

for config in commands-50 aliases-10 mixed-50 overlap-aliases overlap-mixed; do
    validate -c "$bench/$config.yml" --shell bash
done

scenario startup "$work/home" "$bin --version"
scenario commands-50 "$work/home" "$bin --no-version -c $bench/commands-50.yml"
scenario commands-50-dupes "$work/home" "$bin --no-version -c $bench/commands-50-dupes.yml"
scenario commands-50-versions "$work/home" "$bin -c $bench/commands-50.yml"
scenario rc-floor "$work/home" "bash -ic exit"
scenario aliases-10 "$work/home" "$bin --shell bash -c $bench/aliases-10.yml"
scenario mixed-50 "$work/home" "$bin --shell bash -c $bench/mixed-50.yml"
scenario slow-probe "$work/home" "$bin --timeout 0.5 -c $bench/slow-probe.yml"
scenario overlap-commands "$work/home-slow" "$bin --shell bash -c $bench/overlap-commands.yml"
scenario overlap-aliases "$work/home-slow" "$bin --shell bash -c $bench/overlap-aliases.yml"
scenario overlap-mixed "$work/home-slow" "$bin --shell bash -c $bench/overlap-mixed.yml"
PATH=$real_path scenario real-path "$work/home" "$bin --no-version -c $bench/commands-50.yml"

PATH=$real_path

# Syscall counts of the PATH scans (Linux, needs strace): catches accidental repeated scans.
if [ "$uname_s" = Linux ] && [ -n "$strace" ]; then
    for config in commands-50 commands-50-dupes; do
        PATH=$work/bin "$strace" -f -c -o "$work/strace-$config" "$bin" --no-version -c "$bench/$config.yml" >/dev/null 2>&1 || true
        awk -v name="$config" '$NF ~ /^(stat|lstat|newfstatat|statx|access|faccessat2?)$/ { n += $4 == "" ? 0 : $4 }
            END { print name, n + 0 }' "$work/strace-$config" >>"$out/syscalls.txt"
    done
fi

set -- --results "$out" --targets "$bench/targets.json" --binary "$bin"
[ "$check" -eq 1 ] && set -- "$@" --check
[ "$check" -eq 1 ] && [ -f "$baseline" ] && set -- "$@" --baseline "$baseline"
[ -n "$export_file" ] && set -- "$@" --export "$export_file"
"$python" "$bench/compare.py" "$@"
