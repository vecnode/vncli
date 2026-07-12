#!/usr/bin/env bash
set -euo pipefail

if command -v powershell.exe >/dev/null 2>&1; then
  if powershell.exe -NoProfile -Command "Get-Process -Name 'Docker Desktop' -ErrorAction SilentlyContinue | Select-Object -First 1" | grep -q "Docker Desktop"; then
    echo "[INFO] Docker Desktop is already running."
    exit 0
  fi

  if powershell.exe -NoProfile -Command "if (Test-Path 'C:\Program Files\Docker\Docker\Docker Desktop.exe') { Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'; exit 0 } else { exit 1 }"; then
    echo "[INFO] Docker Desktop launch requested."
    exit 0
  fi
fi

echo "[ERROR] Docker Desktop launch is not available on this host."
echo "[INFO] If Docker is installed, start it using your desktop environment."
exit 1
