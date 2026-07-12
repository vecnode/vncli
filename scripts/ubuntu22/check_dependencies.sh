#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# check_dependencies.sh
# Comprehensive dependency checker and installer for vncli CLI.
#
# Checks for: git, curl, jq, docker, rustscan
# Offers automatic installation if any are missing.
#
# Usage:
#   ./check_dependencies.sh
# ---------------------------------------------------------------------------

# Initialize variables
DEPENDENCIES=("git" "curl" "jq" "docker" "rustscan")
declare -A STATUS
declare -a MISSING=()

# ---------------------------------------------------------------------------
# DEPENDENCY CHECK PHASE
# ---------------------------------------------------------------------------

echo "Checking for required dependencies..."
echo ""

for dep in "${DEPENDENCIES[@]}"; do
  echo -n "  Checking $dep... "
  
  if command -v "$dep" &>/dev/null; then
    # Get version info for additional context
    case "$dep" in
      git)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        STATUS["$dep"]="OK"
        ;;
      curl)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        STATUS["$dep"]="OK"
        ;;
      jq)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        STATUS["$dep"]="OK"
        ;;
      docker)
        if docker ps &>/dev/null; then
          VERSION=$(docker --version 2>/dev/null)
          echo "[OK] ($VERSION)"
          STATUS["$dep"]="OK"
        else
          echo "[WARNING] Found but not accessible (daemon may not be running)"
          STATUS["$dep"]="WARNING"
        fi
        ;;
      rustscan)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        STATUS["$dep"]="OK"
        ;;
    esac
  else
    echo "[MISSING]"
    STATUS["$dep"]="MISSING"
    MISSING+=("$dep")
  fi
done

echo ""

# ---------------------------------------------------------------------------
# SUMMARY & INSTALLATION PROMPT
# ---------------------------------------------------------------------------

if [[ ${#MISSING[@]} -eq 0 ]]; then
  echo "✓ All dependencies are installed!"
  echo ""
  exit 0
fi

echo "⚠ The following dependencies are missing or not accessible:"
for dep in "${MISSING[@]}"; do
  echo "  - $dep"
done
echo ""

while true; do
  read -r -p "Would you like to install the missing dependencies? (y/n): " INSTALL_CHOICE
  
  if [[ "$INSTALL_CHOICE" == "y" || "$INSTALL_CHOICE" == "Y" ]]; then
    INSTALL_CHOICE="yes"
    break
  elif [[ "$INSTALL_CHOICE" == "n" || "$INSTALL_CHOICE" == "N" ]]; then
    echo ""
    echo "[INFO] Skipping installation."
    exit 0
  else
    echo "[ERROR] Invalid choice. Please enter 'y' or 'n'."
  fi
done

# ---------------------------------------------------------------------------
# INSTALLATION PHASE
# ---------------------------------------------------------------------------

# Check if running with sudo privileges
if [[ $EUID -ne 0 ]]; then
  echo "[INFO] This script needs to install packages. You may be prompted for your password."
  echo ""
fi

# Determine package manager
if command -v apt-get &>/dev/null; then
  PKG_MANAGER="apt"
  echo "[INFO] Using apt package manager"
  echo ""
  
  # Update package lists
  echo "[INFO] Updating package lists..."
  sudo apt-get update -qq >/dev/null 2>&1 || true
  echo ""
  
  # Install missing dependencies
  for dep in "${MISSING[@]}"; do
    case "$dep" in
      docker)
        echo "[INFO] Installing docker.io (Docker)..."
        sudo apt-get install -y docker.io >/dev/null 2>&1
        echo "[OK] docker.io installed"
        ;;
      rustscan)
        echo "[INFO] Installing rustscan via cargo..."
        if command -v cargo &>/dev/null && cargo install rustscan >/dev/null 2>&1; then
          echo "[OK] rustscan installed"
        else
          echo "[WARNING] rustscan install failed (requires Rust/cargo: https://rustup.rs/)"
        fi
        ;;
      *)
        echo "[INFO] Installing $dep..."
        sudo apt-get install -y "$dep" >/dev/null 2>&1
        echo "[OK] $dep installed"
        ;;
    esac
  done
elif command -v yum &>/dev/null; then
  PKG_MANAGER="yum"
  echo "[INFO] Using yum package manager"
  echo ""
  
  # Install missing dependencies
  for dep in "${MISSING[@]}"; do
    case "$dep" in
      docker)
        echo "[INFO] Installing docker (Docker)..."
        sudo yum install -y docker >/dev/null 2>&1
        echo "[OK] docker installed"
        ;;
      rustscan)
        echo "[INFO] Installing rustscan via cargo..."
        if command -v cargo &>/dev/null && cargo install rustscan >/dev/null 2>&1; then
          echo "[OK] rustscan installed"
        else
          echo "[WARNING] rustscan install failed (requires Rust/cargo: https://rustup.rs/)"
        fi
        ;;
      *)
        echo "[INFO] Installing $dep..."
        sudo yum install -y "$dep" >/dev/null 2>&1
        echo "[OK] $dep installed"
        ;;
    esac
  done
else
  echo "[ERROR] Could not determine package manager (apt or yum required)."
  echo ""
  echo "Manual installation required:"
  for dep in "${MISSING[@]}"; do
    case "$dep" in
      docker)
        echo "  docker: https://docs.docker.com/engine/install/"
        ;;
      rustscan)
        echo "  rustscan: cargo install rustscan (install Rust from https://rustup.rs/)"
        ;;
      *)
        echo "  $dep: Search for '${dep} install' for your Linux distribution"
        ;;
    esac
  done
  exit 1
fi

# ---------------------------------------------------------------------------
# VERIFICATION PHASE
# ---------------------------------------------------------------------------

VERIFICATION_FAILED=0

for dep in "${MISSING[@]}"; do
  echo -n "  Verifying $dep... "
  
  if command -v "$dep" &>/dev/null; then
    case "$dep" in
      git)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        ;;
      curl)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        ;;
      jq)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        ;;
      docker)
        if docker ps &>/dev/null; then
          VERSION=$(docker --version 2>/dev/null)
          echo "[OK] ($VERSION)"
        else
          echo "[WARNING] Installed but daemon may need to be started"
        fi
        ;;
      rustscan)
        VERSION=$("$dep" --version 2>/dev/null | head -n1)
        echo "[OK] ($VERSION)"
        ;;
    esac
  else
    echo "[FAILED]"
    VERIFICATION_FAILED=1
  fi
done

echo ""

if [[ $VERIFICATION_FAILED -eq 0 ]]; then
  echo "✓ All dependencies verified successfully!"
  echo ""
  exit 0
else
  echo "✗ Some dependencies failed verification. Please try manual installation."
  echo ""
  exit 1
fi
