#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# distribute_cli.sh
# Build vn in release mode and package a self-contained, drop-in-runnable
# copy of vncli into a folder on the Desktop - same functionality as
# launching from the repo via run_cli.sh, just relocatable and with no
# Rust toolchain required on the machine it's copied to.
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "[ERROR] cargo not found in PATH."
  echo "Install Rust first: https://rustup.rs/"
  exit 1
fi

RUSTC_INFO=""
if ! RUSTC_INFO="$(rustc -vV 2>/dev/null)"; then
  echo "[ERROR] Failed to run 'rustc -vV'."
  echo "Ensure your Rust toolchain is installed correctly."
  exit 1
fi

RUST_HOST="$(printf '%s\n' "$RUSTC_INFO" | awk '/^host:/{print $2; exit}')"
if [[ -z "$RUST_HOST" ]]; then
  echo "[ERROR] Unable to detect rustc host target."
  echo "Run 'rustc -vV' and ensure Rust is installed correctly."
  exit 1
fi

VN_VERSION="$(grep -m1 '^version' cli/crates/vn/Cargo.toml | sed -E 's/version[[:space:]]*=[[:space:]]*"([^"]+)"/\1/')"
VN_VERSION="${VN_VERSION:-0.0.0}"

echo "[INFO] Building vn CLI (release) for host target $RUST_HOST..."
if ! cargo build --release --manifest-path cli/Cargo.toml -p vn --target "$RUST_HOST"; then
  echo "[ERROR] Build failed."
  exit 1
fi

VN_BIN="cli/target/$RUST_HOST/release/vn"
if [[ ! -x "$VN_BIN" ]]; then
  echo "[ERROR] Binary not found: $VN_BIN"
  exit 1
fi

DIST_NAME="vncli-${VN_VERSION}-${RUST_HOST}"
DIST_DIR="$HOME/Desktop/$DIST_NAME"

if [[ -d "$DIST_DIR" ]]; then
  echo "[INFO] Removing previous distribution at $DIST_DIR..."
  rm -rf "$DIST_DIR"
fi
mkdir -p "$DIST_DIR/cli/target/$RUST_HOST/debug"

echo "[INFO] Packaging distribution at $DIST_DIR..."

# run_cli_dist.sh.tmpl expects the binary at cli/target/<host>/debug/vn
# (matching where run_cli.sh looks in the dev repo); ship the release
# binary in that same slot so no path changes are needed at launch time.
cp "$VN_BIN" "$DIST_DIR/cli/target/$RUST_HOST/debug/vn"

cp README.md "$DIST_DIR/README.md"
cp LICENSE "$DIST_DIR/LICENSE"
cp -r scripts "$DIST_DIR/scripts"
cp -r docker "$DIST_DIR/docker"
cp -r docs "$DIST_DIR/docs"

sed "s/__RUST_HOST__/$RUST_HOST/g" run_cli_dist.sh.tmpl > "$DIST_DIR/run_cli.sh"
chmod +x "$DIST_DIR/run_cli.sh"

echo ""
echo "----------------------------------------"
echo " Distribution ready: $DIST_DIR"
echo " Run it with: $DIST_DIR/run_cli.sh"
echo "----------------------------------------"
