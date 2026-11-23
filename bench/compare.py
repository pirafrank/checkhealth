#!/usr/bin/env python3
"""Summarise bench/run.sh results and check them against bench/targets.json."""

import argparse
import json
import platform
import subprocess
import sys
from pathlib import Path

REGRESSION_LIMIT = 1.25  # fail if more than 25% slower than the baseline


def p95_ms(times):
    ordered = sorted(times)
    index = min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1))))
    return ordered[index] * 1000


def load(results):
    scenarios = {}
    for path in sorted(Path(results).glob("*.json")):
        run = json.loads(path.read_text())["results"][0]
        times = run["times"]
        scenarios[path.stem] = {
            "mean_ms": run["mean"] * 1000,
            "min_ms": run["min"] * 1000,
            "max_ms": run["max"] * 1000,
            "p95_ms": p95_ms(times),
            "runs": len(times),
        }
    return scenarios


def machine():
    cpu = "unknown CPU"
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    except OSError:
        cpu = platform.processor() or cpu
    return f"{cpu}, {platform.system()} {platform.release()}"


def check_targets(data, targets, failures):
    p95 = lambda name: data[name]["p95_ms"]
    for name, limit in targets.get("absolute_ms", {}).items():
        if p95(name) > limit:
            failures.append(f"{name}: p95 {p95(name):.1f} ms > target {limit} ms")
    for name, rule in targets.get("relative", {}).items():
        if "floor" in rule:
            limit = p95(rule["floor"]) + rule["plus_ms"]
        elif "max_of" in rule:
            limit = max(p95(n) for n in rule["max_of"]) + rule["plus_ms"]
        elif "same_as" in rule:
            other = p95(rule["same_as"])
            limit = other + max(other * rule["tolerance_pct"] / 100, rule["min_slack_ms"])
        elif "below_sum_of" in rule:
            limit = sum(p95(n) for n in rule["below_sum_of"]) * rule["factor"]
        else:
            continue
        if p95(name) > limit:
            failures.append(f"{name}: p95 {p95(name):.1f} ms > limit {limit:.1f} ms ({rule['why']})")


def check_baseline(data, baseline, failures):
    for name, old in baseline.get("scenarios", {}).items():
        if name in data and data[name]["p95_ms"] > old["p95_ms"] * REGRESSION_LIMIT + 1.0:
            failures.append(
                f"{name}: p95 {data[name]['p95_ms']:.1f} ms is more than 25% slower than "
                f"the baseline {old['p95_ms']:.1f} ms"
            )


def check_extras(results, binary, targets, failures):
    size = Path(binary).stat().st_size
    budget = targets.get("binary_size_bytes")
    print(f"\nbinary size: {size / 1024:.0f} KiB (budget {budget / 1024:.0f} KiB)" if budget else "")
    if budget and size > budget:
        print(f"warning: binary is larger than the budget", file=sys.stderr)
    syscalls = Path(results, "syscalls.txt")
    if syscalls.exists():
        counts = {k: int(v) for k, v in (l.split() for l in syscalls.read_text().splitlines())}
        print("PATH-scan syscalls (stat/access family): " + ", ".join(f"{k}={v}" for k, v in counts.items()))
        limit = targets.get("max_path_scan_syscalls")
        if limit and counts.get("commands-50", 0) > limit:
            failures.append(f"commands-50: {counts['commands-50']} stat/access calls > budget {limit}")
        if counts.get("commands-50-dupes", 0) > counts.get("commands-50", 0) + 5:
            failures.append("commands-50-dupes does more lookups than commands-50: deduplication broken")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", required=True)
    parser.add_argument("--targets", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--baseline")
    parser.add_argument("--export")
    args = parser.parse_args()

    data = load(args.results)
    targets = json.loads(Path(args.targets).read_text())

    print(f"{'scenario':<24}{'mean ms':>10}{'p95 ms':>10}{'min ms':>10}{'max ms':>10}{'runs':>6}")
    for name, s in data.items():
        print(f"{name:<24}{s['mean_ms']:>10.1f}{s['p95_ms']:>10.1f}{s['min_ms']:>10.1f}{s['max_ms']:>10.1f}{s['runs']:>6}")

    failures = []
    check_extras(args.results, args.binary, targets, failures)
    if args.check:
        check_targets(data, targets, failures)
        if args.baseline:
            check_baseline(data, json.loads(Path(args.baseline).read_text()), failures)
    if args.export:
        Path(args.export).write_text(json.dumps({"machine": machine(), "scenarios": data}, indent=2) + "\n")
        print(f"\nbaseline written to {args.export}")
    if failures:
        print("\nFAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        sys.exit(1)
    if args.check:
        print("\nall targets met")


if __name__ == "__main__":
    main()
