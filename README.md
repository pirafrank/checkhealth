# 🤒 checkhealth

[![CI](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml/badge.svg)](https://github.com/pirafrank/checkhealth/actions/workflows/ci.yml)

A cross-platform utility to check your toolbelt for necessary tools.

## Goals

- Check tools in your toolbelt
- Inform you about what is installed, which version, and where

## Non-Goals

- Install or update tools
- Fix your configuration

## Install

Download the latest binary from the [releases](https://github.com/pirafrank/checkhealth/releases) page and put to folder in `$PATH`.

## Usage

```sh
checkhealth -c /path/to/config.yaml
checkhealth -c /path/to/config.yaml -s zsh
```

## License

MIT
