#!/bin/sh
# Regenerates the benchmark configs. The names are made up and resolved by bench/run.sh, which
# builds a controlled PATH containing `tool-NN` (fast tools), `medium-tool` and `sleep-tool`.
# Missing-on-purpose names are `nonexistent-tool-N`. Run from anywhere; output goes next to
# this script.
set -eu
unset CDPATH
dir=$(cd "$(dirname "$0")" && pwd)

# 50 commands: every fifth one is missing (20%), the rest are present.
commands_50() {
    present=0
    missing=0
    i=1
    while [ "$i" -le 50 ]; do
        if [ $((i % 5)) -eq 0 ]; then
            missing=$((missing + 1))
            printf '  - nonexistent-tool-%d\n' "$missing"
        else
            present=$((present + 1))
            printf '  - tool-%02d\n' "$present"
        fi
        i=$((i + 1))
    done
}

# $1 aliases, $2 functions; only the first four of each are defined by bench/rc.
shell_names() {
    echo "aliases:"
    i=1
    while [ "$i" -le "$1" ]; do
        printf '  - bench-alias-%d\n' "$i"
        i=$((i + 1))
    done
    echo "functions:"
    i=1
    while [ "$i" -le "$2" ]; do
        printf '  - bench-func-%d\n' "$i"
        i=$((i + 1))
    done
}

{
    echo "commands:"
    commands_50
} >"$dir/commands-50.yml"

{
    echo "commands:"
    commands_50
    commands_50
} >"$dir/commands-50-dupes.yml"

shell_names 5 5 >"$dir/aliases-10.yml"

{
    echo "commands:"
    commands_50 | head -30
    shell_names 10 10
} >"$dir/mixed-50.yml"

printf 'commands:\n  - sleep-tool\n' >"$dir/slow-probe.yml"
printf 'commands:\n  - medium-tool-1\n  - medium-tool-2\n  - medium-tool-3\n  - medium-tool-4\n' \
    >"$dir/overlap-commands.yml"
shell_names 3 3 >"$dir/overlap-aliases.yml"
{
    echo "commands:"
    printf '  - medium-tool-1\n  - medium-tool-2\n  - medium-tool-3\n  - medium-tool-4\n'
    shell_names 3 3
} >"$dir/overlap-mixed.yml"
