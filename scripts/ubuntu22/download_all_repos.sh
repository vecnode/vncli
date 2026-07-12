#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# download_all_repos.sh
# Clone all public repositories from a GitHub account.
#
# Usage:
#   ./download_all_repos.sh <username>
#   A username is required - there is no default.
#
# Downloads into: ~/Desktop/git-backup-DD-MM-YYYY-HH-MM-SS/
#
# Requirements (Linux):
#   - git
#   - curl
#   - jq
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# CONFIGURATION
# ---------------------------------------------------------------------------
if [[ -z "${1:-}" ]]; then
  echo "[ERROR] Usage: ./download_all_repos.sh <username>"
  exit 1
fi
GITHUB_USER="$1"
TIMESTAMP="$(date '+%d-%m-%Y-%H-%M-%S')"
if [[ -n "${VNCLI_TARGET_DIR:-}" ]]; then
  TARGET_DIR="$VNCLI_TARGET_DIR"
else
  TARGET_DIR="$HOME/Desktop/git-backup-${TIMESTAMP}"
fi
PER_PAGE=100   # max allowed by GitHub API

# ---------------------------------------------------------------------------
# OS CHECK
# ---------------------------------------------------------------------------
OS="$(uname -s)"
if [[ "$OS" != "Linux" ]]; then
  echo "[ERROR] This script is designed for Linux (detected: $OS)."
  exit 1
fi

# ---------------------------------------------------------------------------
# DEPENDENCY CHECK
# ---------------------------------------------------------------------------
for cmd in git curl jq; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "[ERROR] Required command not found: $cmd"
    echo "        Install it with:  sudo apt install $cmd"
    exit 1
  fi
done

mkdir -p "$TARGET_DIR"
echo "[INFO] Syncing repos for '$GITHUB_USER' into '$TARGET_DIR'"
echo ""

TMP_BASE="$(mktemp -d "${TMPDIR:-/tmp}/vncli-repos-XXXXXX")"
REPO_LIST_FILE="$TMP_BASE/repos.txt"

cleanup() {
  rm -rf "$TMP_BASE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

PAGE=1
while true; do
  JSON_FILE="$TMP_BASE/repos-${PAGE}.json"
  URL="https://api.github.com/users/${GITHUB_USER}/repos?per_page=${PER_PAGE}&page=${PAGE}&type=owner"

  if ! curl -fsSL -H "Accept: application/vnd.github+json" "$URL" >"$JSON_FILE"; then
    echo "[ERROR] API request failed on page $PAGE."
    exit 1
  fi

  BATCH_COUNT="$(jq -r '.[].clone_url' "$JSON_FILE" | sed '/^$/d' | wc -l | tr -d ' ')"
  if [[ "$BATCH_COUNT" == "0" ]]; then
    break
  fi

  if ! jq -r '.[].clone_url' "$JSON_FILE" >>"$REPO_LIST_FILE"; then
    echo "[ERROR] Failed parsing JSON response."
    exit 1
  fi

  COUNT="$(jq 'length' "$JSON_FILE")"
  if (( COUNT < PER_PAGE )); then
    break
  fi

  (( PAGE++ ))
done

CLONED=0
PULLED=0
FAILED=0

if [[ ! -f "$REPO_LIST_FILE" ]]; then
  echo "[INFO] No personal repositories found for '$GITHUB_USER'."
else
  TOTAL="$(wc -l <"$REPO_LIST_FILE" | tr -d ' ')"
  if [[ "$TOTAL" == "0" ]]; then
    echo "[INFO] No personal repositories found for '$GITHUB_USER'."
    echo ""
    echo "----------------------------------------"
    echo " Done. cloned=$CLONED pulled=$PULLED failed=$FAILED"
    echo "----------------------------------------"
    exit 0
  fi

  IDX=0

  while IFS= read -r repo_url; do
    (( IDX++ )) || true
    repo_name=$(basename "$repo_url" .git)
    repo_path="$TARGET_DIR/$repo_name"

    echo "[$IDX/$TOTAL] $repo_name"

    if [[ -d "$repo_path/.git" ]]; then
      if git -C "$repo_path" pull --ff-only --quiet >/dev/null 2>&1; then
        echo "         [OK] pulled"
        (( PULLED++ )) || true
      else
        echo "         [FAIL] pull failed (skipped)"
        (( FAILED++ )) || true
      fi
    else
      if git clone --quiet "$repo_url" "$repo_path" >/dev/null 2>&1; then
        echo "         [OK] cloned"
        (( CLONED++ )) || true
      else
        echo "         [FAIL] clone failed (skipped)"
        (( FAILED++ )) || true
      fi
    fi
  done <"$REPO_LIST_FILE"
fi

echo ""
echo "----------------------------------------"
echo " Done. cloned=$CLONED pulled=$PULLED failed=$FAILED"
echo "----------------------------------------"
