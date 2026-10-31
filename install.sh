#!/bin/sh
# Install rig from source into ~/.local/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/omraval18/rig/main/install.sh | sh
#
# Run it from a checkout to build that checkout instead of cloning again.
# Environment overrides:
#   RIG_INSTALL_DIR  where the binary goes        (default: ~/.local/bin)
#   RIG_REPO         git URL to clone            (default: the GitHub repo)
#   RIG_REF          branch or tag to build      (default: main)

set -eu

REPO="${RIG_REPO:-https://github.com/omraval18/rig.git}"
REF="${RIG_REF:-main}"
DEST="${RIG_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 \
  || die "Rust is needed to build rig. Install it from https://rustup.rs, then run this again."
command -v git >/dev/null 2>&1 || die "git is needed to fetch rig."

# Build the checkout this script sits in, if there is one; otherwise clone a fresh copy.
SRC=""
if [ -f "$(dirname "$0")/Cargo.toml" ] && grep -q 'crates/rig-cli' "$(dirname "$0")/Cargo.toml" 2>/dev/null; then
  SRC="$(cd "$(dirname "$0")" && pwd)"
fi

TMP=""
cleanup() { if [ -n "$TMP" ]; then rm -rf "$TMP"; fi; }
trap cleanup EXIT INT TERM

if [ -z "$SRC" ]; then
  TMP="$(mktemp -d)"
  say "Fetching rig ($REF)..."
  git clone --quiet --depth 1 --branch "$REF" "$REPO" "$TMP/rig"
  SRC="$TMP/rig"
fi

say "Building rig (this takes a minute or two)..."
(cd "$SRC" && cargo build --release --quiet)

mkdir -p "$DEST"
BIN="$DEST/rig"
# Replace rather than overwrite: on macOS, writing over a running or signed binary can kill it.
rm -f "$BIN"
cp "$SRC/target/release/rig" "$BIN"
chmod 755 "$BIN"
# rigd is the same binary; running it under that name means full access.
ln -sf rig "$DEST/rigd"
if [ "$(uname -s)" = "Darwin" ] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - "$BIN" >/dev/null 2>&1 || true
fi

say "Installed $("$BIN" --version) to $BIN, and rigd (full access) next to it"
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) say "Add it to your PATH:  export PATH=\"$DEST:\$PATH\"" ;;
esac
say "Next: rig login codex   (ChatGPT plan), or rig key set anthropic   (API key)"
