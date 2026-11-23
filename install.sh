#!/bin/sh
#
# checkhealth install script
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/pirafrank/checkhealth/main/install.sh | sh
#
# Environment:
#   CHECKHEALTH_TAG          install this release tag (e.g. v0.1.0) instead of the latest
#   CHECKHEALTH_INSTALL_DIR  install here instead of ${HOME}/.local/bin
#
# Notes:
#   Self-contained: needs only curl and tar. The archive is verified against the release
#   checksums manifest when sha256sum or shasum is available. Running it again overwrites the
#   installed binary, so it can also be used to update.
#
set -e

OWNER="pirafrank"
REPO="checkhealth"
BIN_NAME="checkhealth"
INSTALL_DIR="${CHECKHEALTH_INSTALL_DIR:-${HOME}/.local/bin}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)
        echo "Error: Unsupported architecture $ARCH"
        exit 1
        ;;
esac

# Targets must match matrix.jsonc.
case "$OS" in
    Linux)
        if ldd --version 2>&1 | grep -qi "glibc\|gnu libc"; then
            TARGET="${ARCH}-unknown-linux-gnu"
        else
            TARGET="${ARCH}-unknown-linux-musl"
        fi
        ;;
    Darwin)
        TARGET="${ARCH}-apple-darwin"
        ;;
    FreeBSD)
        if [ "$ARCH" != "x86_64" ]; then
            echo "Error: Unsupported architecture $ARCH on FreeBSD"
            exit 1
        fi
        TARGET="x86_64-unknown-freebsd"
        ;;
    *)
        echo "Error: Unsupported OS $OS"
        echo "On Windows, download the .zip from https://github.com/${OWNER}/${REPO}/releases"
        exit 1
        ;;
esac

echo "Detected system: $OS $ARCH"
echo "Target: $TARGET"

for tool in curl tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: $tool is required"
        exit 1
    fi
done

TAG_NAME=${CHECKHEALTH_TAG:-}
if [ -z "$TAG_NAME" ]; then
    echo "Fetching latest version..."
    RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${OWNER}/${REPO}/releases/latest")
    TAG_NAME=$(echo "$RELEASE_JSON" | grep -m 1 '"tag_name":' | sed -E 's/.*"([^" ]+)".*/\1/')
fi
if [ -z "$TAG_NAME" ] || [ "$TAG_NAME" = "null" ]; then
    echo "Error: Could not determine latest release version."
    echo "Download it manually from https://github.com/${OWNER}/${REPO}/releases"
    exit 1
fi
case "$TAG_NAME" in
    *[!a-zA-Z0-9._-]*|.|..) echo "Error: Invalid release tag"; exit 1 ;;
esac
VERSION="${TAG_NAME#v}"
echo "Selected release: $TAG_NAME"

# Asset names match release.yml: <name>-<version>-<target>.tar.gz
FILENAME="${BIN_NAME}-${VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${OWNER}/${REPO}/releases/download/${TAG_NAME}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading ${BASE_URL}/${FILENAME}..."
if ! curl -fL "${BASE_URL}/${FILENAME}" -o "$TMP_DIR/$FILENAME"; then
    echo "Error: Download failed. Please check your internet connection and try again."
    exit 1
fi

# Verify the checksum when a SHA-256 tool is available.
if command -v sha256sum >/dev/null 2>&1; then
    SHA_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    SHA_CMD="shasum -a 256"
elif command -v sha256 >/dev/null 2>&1; then
    # FreeBSD: `sha256 -q` prints only the hash.
    SHA_CMD="sha256 -q"
else
    SHA_CMD=""
    echo "Warning: no sha256sum, shasum or sha256 found, skipping checksum verification."
fi
if [ -n "$SHA_CMD" ]; then
    MANIFEST="${BIN_NAME}-${VERSION}-checksums.txt"
    if ! curl -fsSL "${BASE_URL}/${MANIFEST}" -o "$TMP_DIR/$MANIFEST"; then
        echo "Error: Could not download the checksums manifest."
        exit 1
    fi
    EXPECTED=$(grep "  ${FILENAME}\$" "$TMP_DIR/$MANIFEST" | cut -d ' ' -f 1)
    ACTUAL=$($SHA_CMD "$TMP_DIR/$FILENAME" | cut -d ' ' -f 1)
    if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
        echo "Error: Checksum verification failed for $FILENAME."
        exit 1
    fi
    echo "Checksum verified."
fi

echo "Extracting..."
mkdir "$TMP_DIR/extract"
tar -xzf "$TMP_DIR/$FILENAME" -C "$TMP_DIR/extract"
if [ ! -f "$TMP_DIR/extract/$BIN_NAME" ]; then
    echo "Error: Binary not found in archive."
    exit 1
fi

echo "Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
rm -f "$INSTALL_DIR/$BIN_NAME"
mv "$TMP_DIR/extract/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod +x "$INSTALL_DIR/$BIN_NAME"

echo "Successfully installed $BIN_NAME to $INSTALL_DIR/$BIN_NAME"
echo ""
echo "Run $BIN_NAME --help to get started."
echo ""

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Warning: $INSTALL_DIR is not in your PATH."
        echo "To use $BIN_NAME, add the directory to your PATH:"
        echo ""
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        echo ""
        echo "You can add this to your shell config (e.g., ~/.zshrc or ~/.bashrc)."
        ;;
esac
