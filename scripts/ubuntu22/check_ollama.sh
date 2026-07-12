#!/usr/bin/env bash
set -euo pipefail

if pgrep -x ollama >/dev/null 2>&1; then
  echo "[INFO] Ollama is running."
  exit 0
fi

if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-active --quiet ollama; then
    echo "[INFO] Ollama service is running."
    exit 0
  fi
fi

echo "[WARN] Ollama is NOT running."
exit 1