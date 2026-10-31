#!/bin/sh
# Install rig into ~/.local/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/omraval18/rig/main/install.sh | sh
#
# By default this downloads the latest release for your Mac and checks it against the release's
# SHA256SUMS. If there is no release for your machine, or RIG_FROM_SOURCE=1 is set, it builds from
# source, which needs Rust (https://rustup.rs). Run it from a checkout to build that checkout.
# Environment overrides:
#   RIG_INSTALL_DIR  where the binary goes                (default: ~/.local/bin)
#   RIG_VERSION      release to install, e.g. 0.3.0       (default: the latest release)
#   RIG_FROM_SOURCE  1 to build from source instead
#   RIG_REPO         git URL to clone when building      (default: the GitHub repo)
#   RIG_REF          branch or tag to build from source  (default: main)

set -eu

GITHUB="omraval18/rig"
REPO="${RIG_REPO:-https://github.com/$GITHUB.git}"
REF="${RIG_REF:-main}"
DEST="${RIG_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# The release archive for this machine, e.g. rig-0.3.0-aarch64-apple-darwin.tar.gz, or nothing.
target() {
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) echo aarch64-apple-darwin ;;
    Darwin/x86_64) echo x86_64-apple-darwin ;;
    *) return 1 ;;
  esac
}

sha256_check() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$1" >/dev/null 2>&1
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1" >/dev/null 2>&1
  else
    return 1
  fi
}

# Install a verified release archive. Returns non-zero, and installs nothing, if it cannot.
install_release() {
  tgt="$(target)" || return 1
  command -v curl >/dev/null 2>&1 || return 1
  ver="${RIG_VERSION:-}"
  if [ -z "$ver" ]; then
    ver="$(curl -fsSL "https://api.github.com/repos/$GITHUB/releases/latest" 2>/dev/null \
      | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
  fi
  [ -n "$ver" ] || return 1
  base="https://github.com/$GITHUB/releases/download/v$ver"
  archive="rig-$ver-$tgt.tar.gz"
  work="$(mktemp -d)"
  if curl -fsSL "$base/$archive" -o "$work/$archive" \
    && curl -fsSL "$base/SHA256SUMS" -o "$work/SHA256SUMS"; then
    # Keep only this archive's line, so the check covers exactly the file we downloaded.
    grep " $archive\$" "$work/SHA256SUMS" > "$work/check" 2>/dev/null || { rm -rf "$work"; return 1; }
    if ! (cd "$work" && sha256_check check); then
      say "The download did not match SHA256SUMS; not installing it."
      rm -rf "$work"
      return 1
    fi
    tar -xzf "$work/$archive" -C "$work" || { rm -rf "$work"; return 1; }
    say "Installing rig $ver ($tgt)..."
    install_binary "$work/rig-$ver-$tgt/rig"
    rm -rf "$work"
    return 0
  fi
  rm -rf "$work"
  return 1
}

# Replace rather than overwrite: on macOS, writing over a running or signed binary can kill it.
install_binary() {
  mkdir -p "$DEST"
  rm -f "$DEST/rig"
  cp "$1" "$DEST/rig"
  chmod 755 "$DEST/rig"
  # rigd is the same binary; running it under that name means full access.
  ln -sf rig "$DEST/rigd"
  if [ "$(uname -s)" = "Darwin" ] && command -v codesign >/dev/null 2>&1; then
    codesign --force --sign - "$DEST/rig" >/dev/null 2>&1 || true
  fi
}

# Build from source: the checkout this script sits in, or a fresh clone.
install_from_source() {
  command -v cargo >/dev/null 2>&1 \
    || die "Rust is needed to build rig from source. Install it from https://rustup.rs, then run this again."
  command -v git >/dev/null 2>&1 || die "git is needed to fetch rig."
  SRC=""
  if [ -f "$(dirname "$0")/Cargo.toml" ] && grep -q 'crates/rig-cli' "$(dirname "$0")/Cargo.toml" 2>/dev/null; then
    SRC="$(cd "$(dirname "$0")" && pwd)"
  fi
  TMP=""
  if [ -z "$SRC" ]; then
    TMP="$(mktemp -d)"
    say "Fetching rig ($REF)..."
    git clone --quiet --depth 1 --branch "$REF" "$REPO" "$TMP/rig"
    SRC="$TMP/rig"
  fi
  say "Building rig from source (this takes a minute or two)..."
  (cd "$SRC" && cargo build --release --quiet)
  install_binary "$SRC/target/release/rig"
  [ -z "$TMP" ] || rm -rf "$TMP"
}

if [ "${RIG_FROM_SOURCE:-0}" = "1" ] || [ -f "$(dirname "$0")/crates/rig-cli/Cargo.toml" ]; then
  install_from_source
elif ! install_release; then
  say "No release is available for this machine; building from source instead."
  install_from_source
fi

say "Installed $("$DEST/rig" --version) to $DEST/rig, and rigd (full access) next to it"
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) say "Add it to your PATH:  export PATH=\"$DEST:\$PATH\"" ;;
esac
say "Next: rig login codex   (ChatGPT plan), or rig key set anthropic   (API key)"
