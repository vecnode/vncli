#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "[ERROR] cargo not found in PATH."
  echo "Install Rust first: https://rustup.rs/"
  exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "[ERROR] rustc not found in PATH."
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

echo "[INFO] Building vn CLI for host target $RUST_HOST..."
if ! cargo build --manifest-path cli/Cargo.toml -p vn --target "$RUST_HOST"; then
  echo "[ERROR] Build failed."
  exit 1
fi

VN_BIN="./cli/target/$RUST_HOST/debug/vn"

if [[ ! -x "$VN_BIN" ]]; then
  echo "[ERROR] Binary not found: $VN_BIN"
  exit 1
fi

# RustScan powers 'vn net scan' (open-port scanning). Install it once via cargo
# if it is not already on PATH; failure is non-fatal so the CLI still launches.
if ! command -v rustscan >/dev/null 2>&1; then
  echo "[INFO] rustscan not found. Installing via cargo (one-time)..."
  if ! cargo install rustscan; then
    echo "[WARNING] Failed to install rustscan. 'vn net scan' will not work until it is installed."
  fi
fi

echo "[INFO] Launching vn..."
set +e
"$VN_BIN" "$@"
VN_EXIT=$?
set -e

if [[ "$VN_EXIT" -ne 0 ]]; then
  echo "[ERROR] vn exited with code $VN_EXIT."
fi

exit "$VN_EXIT"
