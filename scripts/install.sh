#!/usr/bin/env bash
# Install tmux-pilot binary from GitHub Releases
# Used by TPM auto-install and standalone installation

set -euo pipefail

REPO="calbertts/tmux-pilot"
INSTALL_DIR="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/bin}"

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    darwin) PLATFORM="apple-darwin" ;;
    linux)  PLATFORM="unknown-linux-gnu" ;;
    *)      echo "❌ Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *)             echo "❌ Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH}-${PLATFORM}"

# Get latest release tag
echo "🔍 Fetching latest release..."
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "❌ Could not determine latest release"
    exit 1
fi
echo "   Latest: ${LATEST}"

# Download
ASSET="pilot-${LATEST}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${LATEST}/${ASSET}"

echo "📦 Downloading ${ASSET}..."
mkdir -p "$INSTALL_DIR"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -fsSL "$URL" -o "$TMPDIR/pilot.tar.gz"; then
    echo "❌ Download failed. URL: $URL"
    echo "   You may need to build from source: cargo build --release"
    exit 1
fi

# Extract and install
tar -xzf "$TMPDIR/pilot.tar.gz" -C "$TMPDIR"
chmod +x "$TMPDIR/pilot"
mv "$TMPDIR/pilot" "$INSTALL_DIR/pilot"

echo "✅ Installed pilot ${LATEST} to ${INSTALL_DIR}/pilot"

# Verify
if "${INSTALL_DIR}/pilot" --version >/dev/null 2>&1; then
    echo "   $("${INSTALL_DIR}/pilot" --version)"
else
    echo "⚠  Binary installed but could not verify. Check ${INSTALL_DIR}/pilot"
fi
