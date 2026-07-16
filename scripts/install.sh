#!/usr/bin/env sh
# LightSandbox install script
# Usage: curl -fsSL https://raw.githubusercontent.com/lipiji/LightSandbox/master/scripts/install.sh | sh
set -e

REPO="lipiji/LightSandbox"
BIN_NAME="lightsandbox-server"
INSTALL_DIR="${LIGHTSANDBOX_INSTALL_DIR:-}"

# ── platform detection ──────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux*)
    case "$ARCH" in
      x86_64)  ARTIFACT="lightsandbox-server-linux-x86_64" ;;
      aarch64|arm64) ARTIFACT="lightsandbox-server-linux-arm64" ;;
      *) echo "error: unsupported arch $ARCH"; exit 1 ;;
    esac
    ;;
  Darwin*)
    case "$ARCH" in
      arm64)   ARTIFACT="lightsandbox-server-macos-arm64" ;;
      x86_64)  ARTIFACT="lightsandbox-server-macos-x86_64" ;;
      *) echo "error: unsupported arch $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "error: unsupported OS $OS — use install.ps1 on Windows"
    exit 1
    ;;
esac

# ── resolve install directory ───────────────────────────────────────────────

if [ -z "$INSTALL_DIR" ]; then
  if [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
  fi
fi

# ── fetch latest version ────────────────────────────────────────────────────

echo "fetching latest release..."
VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/')"

if [ -z "$VERSION" ]; then
  echo "error: could not determine latest release"
  exit 1
fi

echo "installing $BIN_NAME $VERSION..."

URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"
DEST="$INSTALL_DIR/$BIN_NAME"

curl -fsSL --progress-bar "$URL" -o "$DEST"
chmod +x "$DEST"

# ── verify ───────────────────────────────────────────────────────────────────

if ! command -v "$BIN_NAME" >/dev/null 2>&1; then
  echo ""
  echo "installed to $DEST"
  echo "add $INSTALL_DIR to your PATH if it isn't already:"
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
else
  echo ""
  echo "installed $BIN_NAME $VERSION -> $DEST"
fi

echo ""
echo "get started:"
echo "  $BIN_NAME              # start with built-in defaults"
echo "  $BIN_NAME --help       # show options"
