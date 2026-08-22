#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# setup_dotfiles.sh - Copy dotfiles into the Ubuntu 22.04 / 24.04 home directory
#
# Linux counterpart of dotfiles/win11/setup_dotfiles.bat. The Windows script
# elevates once through UAC and then does everything as Administrator. That
# model does not work here: gsettings/dconf must run as the *desktop user*
# against a live session bus, while systemd/apt/sysctl need root. So this runs
# as the normal user and global_configs.sh guards its system-level sections
# behind a non-interactive sudo probe (see that file's HAVE_SUDO handling).
#
# Usage:
#   ./setup_dotfiles.sh [--dry-run]
#
#   --dry-run   Print every change global_configs.sh would make, apply none.
# ---------------------------------------------------------------------------

DOTFILES_SOURCE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SSH_CONFIG_SOURCE="$DOTFILES_SOURCE/ssh/config"
SSH_CONFIG_DEST="$HOME/.ssh/config"

if [[ "${EUID}" -eq 0 ]]; then
  echo "[ERROR] Do not run this as root or with sudo."
  echo "[ERROR] Run it as your normal desktop user; the parts that need root"
  echo "[ERROR] will call sudo themselves (run 'sudo -v' first to pre-authorize)."
  exit 1
fi

echo "[INFO] Running as $(id -un)."

# Ensure .ssh directory exists (0700 - ssh refuses to use a group/world
# readable config directory).
if [[ ! -d "$HOME/.ssh" ]]; then
  mkdir -p "$HOME/.ssh"
  chmod 700 "$HOME/.ssh"
  echo "[INFO] Created directory: $HOME/.ssh"
fi

# Backup existing SSH config if it exists
if [[ -f "$SSH_CONFIG_DEST" ]]; then
  BACKUP_FILE="$(mktemp "${SSH_CONFIG_DEST}.backup_XXXXXX")"
  cp "$SSH_CONFIG_DEST" "$BACKUP_FILE"
  echo "[INFO] Backed up existing SSH config to: $BACKUP_FILE"
fi

# Copy SSH config from dotfiles to destination
if [[ -f "$SSH_CONFIG_SOURCE" ]]; then
  cp "$SSH_CONFIG_SOURCE" "$SSH_CONFIG_DEST"
  chmod 600 "$SSH_CONFIG_DEST"
  echo "[INFO] Copied SSH config to: $SSH_CONFIG_DEST"
else
  echo "[WARNING] SSH config source not found: $SSH_CONFIG_SOURCE"
fi

# ---------------------------------------------------------------------------
# Run global_configs.sh
# ---------------------------------------------------------------------------
GLOBAL_CONFIGS="$DOTFILES_SOURCE/global_configs.sh"

if [[ -f "$GLOBAL_CONFIGS" ]]; then
  echo "[INFO] Running global_configs.sh..."
  if ! bash "$GLOBAL_CONFIGS" "$@"; then
    echo "[ERROR] global_configs.sh exited with an error."
    exit 1
  fi
else
  echo "[WARNING] global_configs.sh not found: $GLOBAL_CONFIGS"
fi

echo "[INFO] Dotfiles setup complete."
exit 0
