#!/usr/bin/env bash
set -euo pipefail

if pgrep -x ollama >/dev/null 2>&1; then
  echo "[INFO] Ollama is already running."
  exit 0
fi

if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-enabled ollama >/dev/null 2>&1 || systemctl status ollama >/dev/null 2>&1; then
    if systemctl start ollama >/dev/null 2>&1; then
      echo "[INFO] Ollama service start requested."
      exit 0
    fi
  fi
fi

if command -v ollama >/dev/null 2>&1; then
  nohup ollama serve >/dev/null 2>&1 &
  echo "[INFO] Ollama launch requested."
  exit 0
fi

echo "[ERROR] Ollama executable not found. Is it installed?"
exit 1