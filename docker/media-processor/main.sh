#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# main.sh
# Start doc-processor API and UI services.
# ---------------------------------------------------------------------------

UI_PORT="${UI_PORT:-8085}"
API_PORT="${API_PORT:-8086}"
API_HOST="${API_HOST:-0.0.0.0}"

echo "[INFO] Starting doc-processor services"
echo "[INFO] UI:  http://localhost:${UI_PORT}"
echo "[INFO] API: http://localhost:${API_PORT}"

python3 /app/docker/media-processor/ui_server.py "${UI_PORT}" &
UI_PID=$!

uvicorn api_server:app --host "${API_HOST}" --port "${API_PORT}" --app-dir /app/docker/media-processor &
API_PID=$!

cleanup() {
  echo "[INFO] Shutting down doc-processor services"
  kill "${UI_PID}" "${API_PID}" >/dev/null 2>&1 || true
}

trap cleanup INT TERM EXIT

wait -n "${UI_PID}" "${API_PID}"
