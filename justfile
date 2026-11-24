# Default recipe: run checkhealth against the sample config
# This must be the first recipe in the file
default: run

# Set default shell based on OS
#set windows-shell := ["bash", "-euc"]

# If on Windows, use Git Bash as the shell.
# using explicit path in case you have multiple Git installations.
set windows-shell := ["C:/Program Files/Git/usr/bin/bash.exe", "-euc"]

# Run checkhealth with the sample config
run *args:
  cargo run -- --config config.sample.yml {{args}}

# List available recipes
list:
  just --list

# Install git hooks
install-hooks:
  git config core.hooksPath hooks

# Install required cargo plugins for recipes to run
install-cargo-plugins:
  cargo install cargo-binstall
  cargo install cargo-edit
  cargo install cargo-deny
  cargo binstall cargo-semver-checks
  cargo install cargo-outdated --locked
  cargo install cargo-audit --locked
  cargo install cargo-llvm-cov
  cargo binstall git-cliff
  python3 -m pip install diff-cover

# Check for errors in all targets
check:
  cargo check --all-targets

# Build the project
build:
  cargo build

# Run all tests
test:
  cargo test --all-features -- --nocapture

# Run only integration tests
test-integration:
  cargo test --all-features --test integration --test panic -- --nocapture

# Get errors and warnings without running the tests
warnings:
  cargo test --no-run 2>&1 | grep -A 5 "warning\|error" || true

# Run the formatter
fmt:
  cargo fmt

# Run the formatter (checks only)
fmt-check:
  cargo fmt -- --check

# Run the linter on default targets (exclude dependencies)
lint:
  cargo clippy -- --no-deps -D warnings

# Run the linter on all targets (exclude dependencies)
lint-all:
  cargo clippy --all-targets --all-features -- --no-deps -D warnings

# Auto-fix compiler suggestions
cargo-fix:
  cargo fix --allow-dirty

# Auto-fix clippy warnings
fix:
  cargo clippy --no-deps --fix --allow-dirty -- -D warnings

# Auto-fix clippy warnings for all targets
fix-all:
  cargo clippy --all-targets --no-deps --fix --allow-dirty -- -D warnings

# Run the formatter and linter on all source files
better: fmt lint-all

# Run pre-commit checks on staged files
pre-commit: fmt-check lint-all

# Run pre-push checks
pre-push: check test

# Run pre-push checks with tags
pre-push-tag:
  hooks_scripts/pre-push-tag.sh

# The following recipe requires git-cliff.
# Generate changelog
changelog version:
  git cliff --tag {{version}} -o CHANGELOG.md
  glow CHANGELOG.md | less

# Prepare release
prepare-release version:
  git cliff --tag {{version}} -o CHANGELOG.md
  nvim CHANGELOG.md
  cargo set-version {{version}}
  git add Cargo.toml Cargo.lock CHANGELOG.md
  git commit -S -m "chore(release): prepare for {{version}}"

# Make and tag a release
make-release version:
  git checkout main
  git tag -s -a v{{version}} -m "{{version}}"

# Build for release
release:
  cargo build --release --locked

# Run cargo artifacts
clean:
  cargo clean

# Clean all downloaded artifacts
clean-dl:
  rm -f *.tar.gz
  rm -f *.tar.bz2
  rm -f *.tar.xz
  rm -f *.zip
  rm -f *.tar
  rm -f *.tgz
  rm -f *.tbz2
  rm -f *.tbz

# Clean all artifacts
clean-all: clean clean-dl

# Generate man pages
man:
  cargo run --example gen_man

# Generate documentation
docs:
  cargo doc --no-deps --open

# Verify that all public items have documentation
docs-verify-public:
  #!/usr/bin/env bash
  set -euo pipefail
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  cargo doc --no-deps 2> "$tmp"
  if grep -q "warning\[missing_docs\]" "$tmp"; then
    echo "Some public items are missing documentation"
    exit 1
  fi
  echo "All public items have documentation"

# Verify that all items have documentation
docs-verify-all:
  cargo clippy --all-targets --no-deps -- -D clippy::missing_docs_in_private_items

# Run benchmarks
bench:
  cargo bench

# Show dependency tree
deps:
  cargo tree

# Update dependencies
update-deps:
  cargo update

# Check for outdated dependencies
outdated-deps:
  cargo outdated --root-deps-only

# Check for security vulnerabilities
audit:
  cargo audit

# Check for license issues
licenses:
  cargo deny check licenses

# Run CI checks
ci: clean fmt-check lint-all build test

# Run compliance checks (audit and license validation)
compliance: audit licenses

# Run code coverage
coverage:
  cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info

# Browse issues on GitHub
issues:
  gh issue list --repo pirafrank/checkhealth --web

# Check coverage of the current diff against the Codecov patch threshold
coverage-diff: coverage
  python3 -m diff_cover.diff_cover_tool lcov.info --compare-branch=origin/main --fail-under=90 --total-percent-float --exclude 'tests/*' 'build.rs'

# Browse coverage report locally
browse-coverage: coverage
  cargo llvm-cov report --html
  python3 -m http.server --directory ./target/llvm-cov/html 8888

# Browse coverage report on Codecov
codecov:
  python3 -m webbrowser "https://app.codecov.io/github/pirafrank/checkhealth"

# Rust MSRV check
msrv:
  version=$(grep 'rust-version =' Cargo.toml | cut -d '=' -f2 | cut -d '"' -f2); \
  cargo +"$version" check --all-targets --locked

# Build the optimized binary used for benchmarks
bench-build:
  cargo build --release --locked

# Run end-to-end timing benchmarks with hyperfine and print a summary
bench-e2e: bench-build
  bench/run.sh

# Same, but fail if a target is exceeded (used by CI and pre-release)
bench-check: bench-build
  bench/run.sh --check

# Save the current results as the new baseline (commit bench/baseline.json deliberately)
bench-baseline: bench-build
  bench/run.sh --export bench/baseline.json

# Smoke run of the end-to-end benchmark scripts (one run, no checks)
bench-smoke: bench-build
  bench/run.sh --runs 1
