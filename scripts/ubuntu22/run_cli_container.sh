#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# run_cli_container.sh
# Build and run the vncli CLI container in interactive mode.
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DOCKERFILE_PATH=""
BUILD_CONTEXT="$REPO_ROOT"
IMAGE_NAME="vncli-cli:latest"
CONTAINER_NAME="vncli-cli-session"
MEDIA_PROCESSOR_MODE=0

detect_dockerfile_path() {
  local candidates=()

  if [[ -n "${VNCLI_CLI_DOCKERFILE:-}" ]]; then
    candidates+=("$VNCLI_CLI_DOCKERFILE")
  fi

  candidates+=(
    "$REPO_ROOT/docker/Dockerfile"
    "$REPO_ROOT/docker/media-processor/Dockerfile"
  )

  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      DOCKERFILE_PATH="$candidate"
      if [[ "$candidate" == "$REPO_ROOT/docker/media-processor/Dockerfile" ]]; then
        MEDIA_PROCESSOR_MODE=1
      fi
      return 0
    fi
  done

  return 1
}

if ! detect_dockerfile_path; then
  echo "[ERROR] No CLI Dockerfile found. Tried:"
  echo "  - $REPO_ROOT/docker/Dockerfile"
  echo "  - $REPO_ROOT/docker/media-processor/Dockerfile"
  echo "[INFO] You can override with VNCLI_CLI_DOCKERFILE=/absolute/path/to/Dockerfile"
  echo "[INFO] For doc-processor ports 8085/8086, run: vn app open doc-processor"
  exit 1
fi

echo "[INFO] Repository root: $REPO_ROOT"
echo "[INFO] Dockerfile: $DOCKERFILE_PATH"
echo "[INFO] Build context: $BUILD_CONTEXT"

if [[ "$MEDIA_PROCESSOR_MODE" -eq 1 ]]; then
  IMAGE_NAME="vncli-media-processor:latest"
  CONTAINER_NAME="vncli-media-processor"
fi

echo "[INFO] Image: $IMAGE_NAME"
echo ""

if ! command -v docker >/dev/null 2>&1; then
  echo "[ERROR] Docker is not available or not in PATH"
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "[ERROR] Docker daemon is not running"
  exit 1
fi

if [[ ! -f "$DOCKERFILE_PATH" ]]; then
  echo "[ERROR] Dockerfile not found: $DOCKERFILE_PATH"
  exit 1
fi

echo "[INFO] Building image..."
docker build -t "$IMAGE_NAME" -f "$DOCKERFILE_PATH" "$BUILD_CONTEXT"

if [[ "$MEDIA_PROCESSOR_MODE" -eq 1 ]]; then
  echo ""
  echo "[INFO] media-processor Dockerfile detected."
  echo "[INFO] Starting media-processor services with port mappings..."

  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  docker run -d --rm --name "$CONTAINER_NAME" -p 127.0.0.1:8085:8085 -p 127.0.0.1:8086:8086 "$IMAGE_NAME" >/dev/null

  echo "[OK] Container started: $CONTAINER_NAME"
  echo "[INFO] UI:  http://localhost:8085"
  echo "[INFO] API: http://localhost:8086"
  echo "[INFO] Health: http://localhost:8086/health"
  echo "[INFO] Logs: docker logs -f $CONTAINER_NAME"
  echo "[INFO] Stop: docker stop $CONTAINER_NAME"
  exit 0
fi

echo ""
echo "[INFO] Starting container in interactive mode..."
echo "[INFO] Container name: $CONTAINER_NAME"
echo "[INFO] Opening shell: /bin/bash"
echo "[INFO] Tools CLI command: bash /app/scripts/tools-cli/alpine/main.sh"
echo "[INFO] vncli CLI command: bash /app/scripts/ubuntu22/main.sh"
echo ""

# Ensure previous container with same name doesn't block startup.
docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true

if [[ -t 0 && -t 1 ]]; then
  exec docker run --rm -it --name "$CONTAINER_NAME" --entrypoint /bin/bash "$IMAGE_NAME"
fi

echo "[INFO] Non-interactive terminal detected (e.g., TUI background process)."
echo "[INFO] Starting detached container so you can attach manually."

docker run -d --rm --name "$CONTAINER_NAME" --entrypoint /bin/bash "$IMAGE_NAME" -lc "while true; do sleep 3600; done" >/dev/null

echo "[OK] Container started: $CONTAINER_NAME"
echo "[INFO] Open shell with: docker exec -it $CONTAINER_NAME /bin/bash"
