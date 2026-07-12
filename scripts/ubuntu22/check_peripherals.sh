#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# check_peripherals.sh
# Print currently connected peripherals, one per line.
# ---------------------------------------------------------------------------

if ! command -v lsusb >/dev/null 2>&1; then
  echo "[ERROR] lsusb is not available. Install usbutils to use this script."
  exit 1
fi

# Keep output compact and similar to the Windows helper: one device per line.
lsusb \
  | sed -E 's/^Bus [0-9]+ Device [0-9]+: ID [0-9a-fA-F]{4}:[0-9a-fA-F]{4}[[:space:]]+//' \
  | sed '/^[[:space:]]*$/d' \
  | sort -u
