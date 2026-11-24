# Changelog

All notable changes to the project will be documented in this file.

## [0.2.0-rc.2] - 2026-11-24

### 🚀 Features

- Flat CLI, config loading and exit codes
- Check commands, report output and exit codes
- Best-effort version probing with timeout and process-group kill
- Concurrent runner with bounded probe pool
- Check aliases and functions with a single shell invocation
- End-to-end benchmarks with hyperfine, bench workflow and CI smoke run
- Installation scripts
- Show headings and group output
- Auto-detect piped output and added option to export in tsv, csv, or json
- [**breaking**] Add auto pager and stop choosing output format by pipe

### 🐛 Bug Fixes

- Run children in their own session so interactive shells do not stop on the tty

### 📚 Documentation

- Rewrite README and AGENTS.md; switch YAML crate to serde_norway

### 🧪 Testing

- Fix Windows failures and dead-code warning

### 🔧 Setup & Quality

- *(justfile)* Specify which bash to use in windows

### ⚙️ Miscellaneous Tasks

- Scaffold checkhealth from rust-cli-boilerplate
- Justfile fixes

