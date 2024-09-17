#!/usr/bin/env -S just

# Task to build the project
build:
    cargo build

# Task to build the project in release mode
release:
    cargo build --release

# Task to pack the project for GitHub release
pack:
    ./pack.sh

# Task to run the project with the specified number of threads
run:
    cargo run -- -c config.sample.yml

test:
    cargo test --verbose

# Default task to run the project with the default number of threads
default:
    just run
