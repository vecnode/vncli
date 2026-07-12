#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# download_all_orgs.sh
# Clone all public repositories from a list of GitHub organizations.
#
# Usage:
#   ./download_all_orgs.sh <org1> [org2 org3 ...]
#   At least one organization name is required - there is no default list.
#
# Downloads into: ~/Desktop/git-backup-orgs-DD-MM-YYYY-HH-MM-SS/
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# CONFIGURATION
# ---------------------------------------------------------------------------
if [[ "$#" -eq 0 ]]; then
	echo "[ERROR] Usage: ./download_all_orgs.sh <org1> [org2 org3 ...]"
	exit 1
fi
ORG_LIST=("$@")
TIMESTAMP="$(date '+%d-%m-%Y-%H-%M-%S')"
if [[ -n "${VNCLI_TARGET_DIR:-}" ]]; then
	TARGET_DIR="$VNCLI_TARGET_DIR"
else
	TARGET_DIR="$HOME/Desktop/git-backup-orgs-${TIMESTAMP}"
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

fetch_org_repos() {
	local org_name="$1"
	local page=1
	local list_file="$2"

	while true; do
		local url="https://api.github.com/orgs/${org_name}/repos?per_page=${PER_PAGE}&page=${page}&type=public"
		local json_file
		json_file="$TMP_BASE/${org_name}-${page}.json"

		if ! curl -fsSL -H "Accept: application/vnd.github+json" "$url" >"$json_file"; then
			echo "         [FAIL] API request failed for $org_name (page $page)"
			(( FAILED++ )) || true
			return 1
		fi

		local batch_count
		batch_count="$(jq -r '.[].clone_url' "$json_file" | sed '/^$/d' | wc -l | tr -d ' ')"
		[[ "$batch_count" == "0" ]] && break

		if ! jq -r '.[].clone_url' "$json_file" >>"$list_file"; then
			echo "         [FAIL] Failed parsing JSON for $org_name"
			(( FAILED++ )) || true
			return 1
		fi

		local count
		count=$(jq 'length' "$json_file")
		(( count < PER_PAGE )) && break

		(( page++ ))
	done
}

mkdir -p "$TARGET_DIR"
echo "[INFO] Syncing hardcoded organizations into '$TARGET_DIR'"

CLONED=0
PULLED=0
FAILED=0
ORGS_COUNT=0
ORG_REPOS_TOTAL=0
ORGS_TOTAL=${#ORG_LIST[@]}

TMP_BASE="$(mktemp -d "${TMPDIR:-/tmp}/vncli-orgs-XXXXXX")"
cleanup() {
  rm -rf "$TMP_BASE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

for org_name in "${ORG_LIST[@]}"; do
	(( ORGS_COUNT++ )) || true

	org_dir="$TARGET_DIR/$org_name"
	mkdir -p "$org_dir"

	echo ""
	echo "[ORG $ORGS_COUNT/$ORGS_TOTAL] $org_name"

	org_list_file="$TMP_BASE/${org_name}-repos.txt"
	rm -f "$org_list_file"

	if ! fetch_org_repos "$org_name" "$org_list_file"; then
		continue
	fi

	if [[ ! -f "$org_list_file" ]]; then
		echo "         no public repositories"
		continue
	fi

	org_total="$(wc -l <"$org_list_file" | tr -d ' ')"
	if [[ "$org_total" == "0" ]]; then
		echo "         no public repositories"
		continue
	fi

	(( ORG_REPOS_TOTAL += org_total )) || true
	org_idx=0

	while IFS= read -r repo_url; do
		(( org_idx++ )) || true
		repo_name=$(basename "$repo_url" .git)
		repo_path="$org_dir/$repo_name"

		echo "         [$org_idx/$org_total] $repo_name"

		if [[ -d "$repo_path/.git" ]]; then
			if git -C "$repo_path" pull --ff-only --quiet >/dev/null 2>&1; then
				echo "                  [OK] pulled"
				(( PULLED++ )) || true
			else
				echo "                  [FAIL] pull failed (skipped)"
				(( FAILED++ )) || true
			fi
		else
			if git clone --quiet "$repo_url" "$repo_path" >/dev/null 2>&1; then
				echo "                  [OK] cloned"
				(( CLONED++ )) || true
			else
				echo "                  [FAIL] clone failed (skipped)"
				(( FAILED++ )) || true
			fi
		fi
	done <"$org_list_file"
done

echo ""
echo "----------------------------------------"
echo " Done. cloned=$CLONED pulled=$PULLED failed=$FAILED"
echo "       orgs=$ORGS_COUNT org_repos=$ORG_REPOS_TOTAL"
echo "----------------------------------------"

