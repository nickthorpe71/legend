#!/usr/bin/env bash
set -euo pipefail

REPO="nickthorpe71/legend"
INSTALL_DIR="${LEGEND_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
case "$(uname -s)" in
  Linux*)          OS="linux" ;;
  Darwin*)         OS="macos" ;;
  MINGW*|MSYS*|CYGWIN*) OS="windows" ;;
  *)               echo "Error: Unsupported OS $(uname -s)." >&2; exit 1 ;;
esac

# Detect architecture
case "$(uname -m)" in
  x86_64|amd64)  ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *)             echo "Error: Unsupported architecture $(uname -m)." >&2; exit 1 ;;
esac

if [ "$OS" = "windows" ]; then
  BINARY="legend-${OS}-${ARCH}.exe"
else
  BINARY="legend-${OS}-${ARCH}"
fi

echo "Detecting platform: ${OS} ${ARCH}"

# Get latest release tag
echo "Fetching latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*: "\(.*\)".*/\1/')

if [ -z "$TAG" ]; then
  echo "Error: Could not determine latest release." >&2
  exit 1
fi

echo "Latest release: ${TAG}"

URL="https://github.com/${REPO}/releases/download/${TAG}/${BINARY}"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
echo "Downloading ${BINARY}..."
if [ "$OS" = "windows" ]; then
  DEST="${INSTALL_DIR}/legend.exe"
else
  DEST="${INSTALL_DIR}/legend"
fi

curl -fsSL "$URL" -o "$DEST"
chmod +x "$DEST"

echo "Installed legend ${TAG} to ${DEST}"

# Check if install dir is in PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  echo ""
  echo "WARNING: ${INSTALL_DIR} is not in your PATH."
  echo "Add it with:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo "Or add that line to your ~/.bashrc or ~/.zshrc"
fi
