#!/usr/bin/env sh
set -eu

REPO="Stretchyfish/tui3"
INSTALL_DIR="${TUI3_INSTALL_DIR:-$HOME/.local/bin}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  OS_KEY="linux" ;;
  Darwin) OS_KEY="macos" ;;
  *)
    echo "Unsupported OS: $OS" >&2
    echo "Build from source instead: cargo install --git https://github.com/$REPO" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH_KEY="x86_64" ;;
  aarch64|arm64) ARCH_KEY="aarch64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    echo "Build from source instead: cargo install --git https://github.com/$REPO" >&2
    exit 1
    ;;
esac

ASSET="tui3-${OS_KEY}-${ARCH_KEY}.tar.gz"

if [ -n "${TUI3_VERSION:-}" ]; then
  TAG="$TUI3_VERSION"
  URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"
else
  URL="https://github.com/$REPO/releases/latest/download/$ASSET"
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading tui3 ($OS_KEY/$ARCH_KEY)..."
if ! curl -fsSL "$URL" -o "$TMP_DIR/$ASSET"; then
  echo "Failed to download $URL" >&2
  echo "Ensure the repository is public and a release has been published for $OS_KEY/$ARCH_KEY." >&2
  exit 1
fi

tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

if [ ! -f "$TMP_DIR/tui3" ]; then
  echo "Archive did not contain a tui3 binary." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
install -m 755 "$TMP_DIR/tui3" "$INSTALL_DIR/tui3"

echo "Installed tui3 to $INSTALL_DIR/tui3"
case ":${PATH:-}:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo "Add $INSTALL_DIR to your PATH to run 'tui3' directly:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
