#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# Global Ubuntu 22.04 / 24.04 Configs
#
# Linux counterpart of dotfiles/win11/global_configs.ps1. Run as the current
# desktop user - no hardcoded usernames; gsettings writes land in whoever's
# dconf profile is running this.
#
# Two structural differences from the Windows script:
#
#   1. There is no single "elevate once and do everything" mode. gsettings and
#      dconf must run as the desktop user against a live session bus, while
#      systemd/apt/sysctl need root. System-level sections are therefore
#      guarded by $HAVE_SUDO exactly the way the .ps1 guards HKLM writes
#      behind $isAdmin, and print the same style of skip warning.
#   2. sudo must never block on a password prompt. `vn run` spawns this with
#      piped stdout/stderr, so an interactive prompt would hang the TUI panel
#      with nothing visible to type into. We probe with `sudo -n true` and skip
#      rather than prompt. Run `sudo -v` first (or run from a plain terminal)
#      to apply the system-level sections.
#
# Usage:
#   ./global_configs.sh [--dry-run]
# ---------------------------------------------------------------------------

usage() {
  echo "Usage: global_configs.sh [--dry-run]"
  echo ""
  echo "  --dry-run   Print every change that would be made and apply none."
  echo "  -h, --help  Show this help."
}

DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[ERROR] Unknown argument: $arg"; usage; exit 1 ;;
  esac
done

# ---------------------------------------------------------------------------
# Packages removed by this script.
#
# The Windows script's $bloatApps equivalent. Kept deliberately conservative
# and visible at the top of the file: this purges, so nothing goes in here
# that could plausibly be somebody's daily driver. Extras are listed commented
# out rather than enabled by default - uncomment what you actually want gone.
# ---------------------------------------------------------------------------
BLOAT_PACKAGES=(
  "gnome-mahjongg"
  "gnome-mines"
  "gnome-sudoku"
  "gnome-sushi"
  # "aisleriot"
  # "thunderbird"
  # "rhythmbox"
  # "libreoffice-core"
  # "simple-scan"
)

log()  { printf '[INFO] %s\n' "$*"; }
warn() { printf '[WARNING] %s\n' "$*"; }

# Summary line for a gsettings-only section. Silent when there is no session
# bus, so a headless run doesn't claim to have applied desktop settings that
# gset() actually skipped.
log_desktop() {
  [[ ${HAS_SESSION:-0} -eq 1 ]] && log "$*"
  return 0
}

# Run a command, or print it under --dry-run. Failures are reported and
# skipped rather than fatal - the Windows script's -ErrorAction
# SilentlyContinue, so one missing unit or package can't abort the baseline.
run() {
  if [[ $DRY_RUN -eq 1 ]]; then
    printf '[DRY-RUN] %s\n' "$*"
    return 0
  fi
  if ! "$@"; then
    warn "Command failed (continuing): $*"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Host detection
# ---------------------------------------------------------------------------
DISTRO_ID=""
DISTRO_VERSION=""
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO_ID="${ID:-}"
  DISTRO_VERSION="${VERSION_ID:-}"
fi

if [[ "$DISTRO_ID" != "ubuntu" ]]; then
  warn "This baseline targets Ubuntu; detected '${DISTRO_ID:-unknown}'. Continuing, but expect skipped sections."
else
  case "$DISTRO_VERSION" in
    22.04|24.04) log "Detected Ubuntu $DISTRO_VERSION." ;;
    *) warn "Detected Ubuntu ${DISTRO_VERSION:-unknown}; this baseline is written for 22.04 and 24.04." ;;
  esac
fi

# A GNOME/dconf session is required for every gsettings section. On a headless
# server there is no session bus, so those sections are skipped wholesale
# instead of erroring once per key.
HAS_SESSION=0
if command -v gsettings >/dev/null 2>&1 &&
   { [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] || [[ -S "/run/user/$(id -u)/bus" ]]; }; then
  HAS_SESSION=1
else
  warn "No GNOME session bus detected - all desktop (gsettings) sections will be skipped."
fi

# Non-interactive sudo probe. See the header comment for why this must never
# fall through to an interactive prompt.
HAVE_SUDO=0
SUDO_SIMULATED=0
if sudo -n true >/dev/null 2>&1; then
  HAVE_SUDO=1
fi
if [[ $DRY_RUN -eq 1 && $HAVE_SUDO -eq 0 ]]; then
  warn "No cached sudo credentials - dry-run will still show the root-level changes."
  HAVE_SUDO=1
  SUDO_SIMULATED=1
fi
if [[ $HAVE_SUDO -eq 0 ]]; then
  warn "No cached sudo credentials - system-level sections will be skipped. Run 'sudo -v' first to apply them."
fi

# ---------------------------------------------------------------------------
# Restore script
#
# The Windows script has no undo. On Linux the previous value of every
# gsettings key is one `gsettings get` away, so we record them into a
# runnable restore script before overwriting. Covers dconf only - package
# removals, masked units and /etc drop-ins are not reverted by it.
# ---------------------------------------------------------------------------
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/vncli"
RESTORE_FILE="$STATE_DIR/dotfiles-restore-$(date +%Y%m%d-%H%M%S).sh"
if [[ $DRY_RUN -eq 0 && $HAS_SESSION -eq 1 ]]; then
  mkdir -p "$STATE_DIR"
  {
    echo "#!/usr/bin/env bash"
    echo "# Restores the gsettings values replaced by vncli global_configs.sh"
    echo "# on $(date -Is). Does not undo package removals, masked systemd"
    echo "# units, or files written under /etc."
    echo "set -euo pipefail"
    echo ""
  } > "$RESTORE_FILE"
  chmod +x "$RESTORE_FILE"
fi

# Set one gsettings key, recording the outgoing value in the restore script.
# Skips silently when the schema or key does not exist, which is how the
# 22.04 (GNOME 42) / 24.04 (GNOME 46) schema differences are absorbed rather
# than branched on per-release.
gset() {
  local schema="$1" key="$2" value="$3"
  [[ $HAS_SESSION -eq 1 ]] || return 0

  if ! gsettings writable "$schema" "$key" >/dev/null 2>&1; then
    warn "Skipped $schema $key - not present on this GNOME version."
    return 0
  fi

  local current=""
  current="$(gsettings get "$schema" "$key" 2>/dev/null || true)"
  if [[ $DRY_RUN -eq 0 && -n "$current" && -f "$RESTORE_FILE" ]]; then
    printf 'gsettings set %s %s %q\n' "$schema" "$key" "$current" >> "$RESTORE_FILE"
  fi

  run gsettings set "$schema" "$key" "$value"
}

# Disable a systemd unit only if it is actually installed. Unit names moved
# between releases (ubuntu-advantage-tools on 22.04 -> ubuntu-pro-client on
# 24.04), so presence is checked instead of hardcoding per-version lists.
disable_unit_if_present() {
  local unit="$1"
  if systemctl list-unit-files "$unit" --no-legend 2>/dev/null | grep -q .; then
    run sudo systemctl disable --now "$unit"
    log "Disabled unit: $unit"
  fi
}

# Write a root-owned file. Kept separate from run() because it needs a pipe.
write_root_file() {
  local path="$1" content="$2"
  if [[ $DRY_RUN -eq 1 ]]; then
    printf '[DRY-RUN] write %s:\n' "$path"
    printf '%s\n' "$content" | sed 's/^/[DRY-RUN]   /'
    return 0
  fi
  printf '%s\n' "$content" | sudo tee "$path" >/dev/null
}

# ---------------------------------------------------------------------------
# Disable external search providers (the Bing-in-Start-menu equivalent)
# GNOME Shell's Activities search forwards queries to "search providers" -
# including online-account-backed ones - so a keystroke in the overview can
# leave the machine. disable-external makes the overview local-only, which is
# both the BingSearchEnabled=0 and the Start-menu-suggestions analog.
# ---------------------------------------------------------------------------
gset org.gnome.desktop.search-providers disable-external true
log_desktop "Disabled external (online) search providers in the GNOME overview."

# ---------------------------------------------------------------------------
# Disable GNOME Software's promotional / auto-download surfaces
# The closest thing Ubuntu has to the Settings-app "suggested content" ads.
# ---------------------------------------------------------------------------
gset org.gnome.software download-updates false
gset org.gnome.software download-updates-notify false
gset org.gnome.software show-ratings false
log_desktop "Disabled GNOME Software background downloads and rating surfaces."

# ---------------------------------------------------------------------------
# Disable crash reporting and upload (whoopsie / apport)
# whoopsie is the daemon that ships crash dumps to Canonical's error tracker;
# apport is what generates them. Together these are the practical equivalent
# of both the SIUF feedback prompts and Defender's automatic sample
# submission: nothing about a local failure leaves the machine.
# Requires sudo.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  disable_unit_if_present "whoopsie.service"
  disable_unit_if_present "whoopsie.path"
  disable_unit_if_present "apport.service"
  if [[ -f /etc/default/apport ]]; then
    run sudo sed -i 's/^enabled=.*/enabled=0/' /etc/default/apport
    log "Set enabled=0 in /etc/default/apport."
  fi
  log "Disabled crash reporting and upload (whoopsie, apport)."
else
  warn "Skipped crash-reporting disable - requires sudo. Run 'sudo -v' and re-run to apply."
fi

gset com.ubuntu.update-notifier show-apport-crashes false

# ---------------------------------------------------------------------------
# Opt out of diagnostic telemetry
# The AllowTelemetry=Basic equivalent. ubuntu-report is the installer/desktop
# metrics channel; popularity-contest reports installed-package statistics on
# a timer. Neither has a "minimum level" - both are simply off.
# ---------------------------------------------------------------------------
if command -v ubuntu-report >/dev/null 2>&1; then
  run ubuntu-report -f send no
  log "Opted out of ubuntu-report diagnostic telemetry."
else
  log "ubuntu-report not installed - nothing to opt out of."
fi

if [[ $HAVE_SUDO -eq 1 ]]; then
  if [[ -f /etc/popularity-contest.conf ]]; then
    run sudo sed -i 's/^PARTICIPATE=.*/PARTICIPATE="no"/' /etc/popularity-contest.conf
    log "Opted out of popularity-contest package reporting."
  fi
  disable_unit_if_present "popularity-contest.timer"
else
  warn "Skipped popularity-contest opt-out - requires sudo."
fi

# ---------------------------------------------------------------------------
# Disable the phone-home / nag timers
#
# This is the equivalent of BOTH Windows sections that dealt with license
# validation: the scheduled-task disable AND the sppsvc outbound firewall
# block. Ubuntu has no activation service to firewall off - what it has is a
# set of timers that periodically contact Canonical for Pro/ESM contract
# state and MOTD advertising copy. Masking those units IS the block; there is
# no per-process outbound rule to add, and ufw cannot express one.
# Requires sudo.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  NAG_UNITS=(
    "apt-news.service"
    "esm-cache.service"
    "ua-timer.timer"
    "ua-timer.service"
    "ua-messaging.timer"
    "ua-messaging.service"
    "ua-reboot-cmds.service"
    "ubuntu-advantage.service"
    "motd-news.timer"
    "motd-news.service"
  )
  for unit in "${NAG_UNITS[@]}"; do
    disable_unit_if_present "$unit"
  done

  if [[ -f /etc/default/motd-news ]]; then
    run sudo sed -i 's/^ENABLED=.*/ENABLED=0/' /etc/default/motd-news
    log "Set ENABLED=0 in /etc/default/motd-news."
  fi

  # The APT hook that fetches Pro/ESM advertising during every apt run.
  if [[ -f /etc/apt/apt.conf.d/20apt-esm-hook.conf ]]; then
    write_root_file /etc/apt/apt.conf.d/20apt-esm-hook.conf \
      "// Emptied by vncli dotfiles: suppresses Pro/ESM advertising during apt runs."
    log "Neutralized the apt ESM advertising hook."
  fi

  log "Disabled Canonical contract-check and MOTD advertising timers."
else
  warn "Skipped phone-home timer disable - requires sudo. Run 'sudo -v' and re-run to apply."
fi

# ---------------------------------------------------------------------------
# Remove preinstalled bloatware
# The $bloatApps equivalent. Only packages that are actually installed are
# touched, and each is held afterwards so a later dist-upgrade cannot pull it
# back in - the same intent as DisableWindowsConsumerFeatures.
# Requires sudo.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  for pkg in "${BLOAT_PACKAGES[@]}"; do
    if dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "ok installed"; then
      run sudo apt-get purge -y "$pkg"
      run sudo apt-mark hold "$pkg"
      log "Removed and held $pkg."
    fi
  done
  run sudo apt-get autoremove -y
else
  warn "Skipped bloatware removal - requires sudo."
fi

# ---------------------------------------------------------------------------
# Stop snaps from silently refreshing
# snapd refreshes installed snaps on its own schedule with no prompt, which is
# the closest analog to CloudContent reinstalling apps behind your back.
# refresh.hold accepts at most ~90 days, so this is a renewable hold rather
# than a permanent off switch - re-run this script to extend it.
# Requires sudo.
# ---------------------------------------------------------------------------
if ! command -v snap >/dev/null 2>&1; then
  log "snapd not installed - no auto-refresh to hold."
elif [[ $HAVE_SUDO -eq 1 ]]; then
  HOLD_UNTIL="$(date -u -d '+60 days' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || true)"
  if [[ -n "$HOLD_UNTIL" ]]; then
    run sudo snap set system refresh.hold="$HOLD_UNTIL"
    log "Held snap auto-refresh until $HOLD_UNTIL (re-run to extend)."
  fi
else
  warn "Skipped snap refresh hold - requires sudo."
fi

# ---------------------------------------------------------------------------
# Cloud account integration (the OneDrive-block equivalent)
# Stock Ubuntu ships no always-on cloud sync client, so there is no
# DisableFileSyncNGSC to set. What exists is GNOME Online Accounts: once an
# account is added, its files/calendar/search integration mounts into the
# desktop. External search providers are already disabled above; this section
# reports any configured account rather than deleting it, since that would be
# destroying credentials the Windows script never touched.
# ---------------------------------------------------------------------------
GOA_ACCOUNTS="$HOME/.config/goa-1.0/accounts.conf"
if [[ -s "$GOA_ACCOUNTS" ]]; then
  warn "GNOME Online Accounts are configured ($GOA_ACCOUNTS). Remove them in Settings > Online Accounts to fully disable cloud integration."
else
  log "No GNOME Online Accounts configured - no cloud sync integration active."
fi

# ---------------------------------------------------------------------------
# File manager: show hidden files, full path, local-only thumbnails
# Direct equivalents of the Explorer section. Note that "show file
# extensions" has no counterpart - Linux filenames always show in full.
# Local-only thumbnails is the DisableThumbnailsOnNetworkFolders analog:
# nautilus will not generate previews for files on a remote mount.
# ---------------------------------------------------------------------------
gset org.gnome.nautilus.preferences show-hidden-files true
gset org.gtk.Settings.FileChooser show-hidden true
gset org.gnome.nautilus.preferences always-use-location-entry true
gset org.gnome.nautilus.preferences show-image-thumbnails "'local-only'"
log_desktop "File manager: hidden files visible, editable full-path bar, local-only thumbnails."

# ---------------------------------------------------------------------------
# Disable the screen reader and its hotkey
# Orca is GNOME's screen reader; Super+Alt+S is its default shortcut and is
# very easy to trigger by accident - the Win+Enter / Narrator situation
# exactly. Clearing the media-key binding disables the shortcut itself.
# ---------------------------------------------------------------------------
gset org.gnome.desktop.a11y.applications screen-reader-enabled false
gset org.gnome.settings-daemon.plugins.media-keys screenreader "[]"
log_desktop "Disabled the Orca screen reader and its Super+Alt+S hotkey."

# ---------------------------------------------------------------------------
# Never sleep, never blank the screen
# The powercfg standby-timeout / monitor-timeout equivalents, for both AC and
# battery. idle-delay 0 means "never blank"; sleep-inactive-*-type 'nothing'
# means automatic suspend never fires. Manual suspend still works - the
# Windows script disabled the timeouts, not the capability.
# ---------------------------------------------------------------------------
gset org.gnome.settings-daemon.plugins.power sleep-inactive-ac-type "'nothing'"
gset org.gnome.settings-daemon.plugins.power sleep-inactive-battery-type "'nothing'"
gset org.gnome.settings-daemon.plugins.power sleep-inactive-ac-timeout 0
gset org.gnome.settings-daemon.plugins.power sleep-inactive-battery-timeout 0
gset org.gnome.settings-daemon.plugins.power idle-dim false
gset org.gnome.desktop.session idle-delay 0
log_desktop "Sleep and screen-blank timeouts disabled (AC and battery)."

# ---------------------------------------------------------------------------
# Ignore the lid switch
# No Windows counterpart, but it is the other half of "never sleep" on a
# laptop: without it, closing the lid suspends regardless of the timeouts.
# Written as a drop-in so the packaged logind.conf stays pristine.
# Requires sudo.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  run sudo mkdir -p /etc/systemd/logind.conf.d
  write_root_file /etc/systemd/logind.conf.d/99-vncli.conf \
"# Written by vncli dotfiles: never suspend on lid close.
[Login]
HandleLidSwitch=ignore
HandleLidSwitchDocked=ignore
HandleLidSwitchExternalPower=ignore"
  log "Lid-close suspend disabled (takes effect on next login/reboot)."
else
  warn "Skipped lid-switch setting - requires sudo."
fi

# ---------------------------------------------------------------------------
# Disable hibernation
# powercfg /hibernate off equivalent. Ubuntu does not enable hibernation by
# default, but masking the targets makes that explicit and prevents anything
# from triggering a suspend-to-disk. suspend.target is deliberately NOT
# masked - manual sleep stays available, same as on the Windows side.
# Requires sudo.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  run sudo systemctl mask hibernate.target hybrid-sleep.target
  log "Hibernation disabled (hibernate.target, hybrid-sleep.target masked)."
else
  warn "Skipped hibernation disable - requires sudo."
fi

# ---------------------------------------------------------------------------
# Disable the screensaver and screen lock
# ScreenSaveActive=0 / ScreenSaverIsSecure=0 equivalents.
# ---------------------------------------------------------------------------
gset org.gnome.desktop.screensaver idle-activation-enabled false
gset org.gnome.desktop.screensaver lock-enabled false
gset org.gnome.desktop.lockdown disable-lock-screen true
log_desktop "Screensaver and automatic screen lock disabled."

# ---------------------------------------------------------------------------
# Set the desktop background to solid black
# Simpler than the Windows path: GNOME renders a solid colour natively when
# picture-uri is empty, so there is no 1x1 BMP to generate and no
# SystemParametersInfo call to force a repaint - dconf changes apply live.
# picture-uri-dark is set too so the background does not come back when the
# dark style is active (GNOME 42+, i.e. both 22.04 and 24.04).
# ---------------------------------------------------------------------------
gset org.gnome.desktop.background picture-uri "''"
gset org.gnome.desktop.background picture-uri-dark "''"
gset org.gnome.desktop.background primary-color "'#000000'"
gset org.gnome.desktop.background secondary-color "'#000000'"
gset org.gnome.desktop.background color-shading-type "'solid'"
gset org.gnome.desktop.screensaver picture-uri "''"
gset org.gnome.desktop.screensaver primary-color "'#000000'"
log_desktop "Desktop background set to solid black (applied immediately)."

# ---------------------------------------------------------------------------
# Raise inotify watch and file-descriptor limits
# The LongPathsEnabled analog: not a path-length limit (Linux has none worth
# raising) but the equivalent class of "the packaged ceiling is too low for
# large builds". Big source trees, Unreal, and file-watching toolchains all
# exhaust the default inotify watch budget and hit "too many open files".
# Requires sudo. Applied immediately via sysctl --system.
# ---------------------------------------------------------------------------
if [[ $HAVE_SUDO -eq 1 ]]; then
  write_root_file /etc/sysctl.d/99-vncli.conf \
"# Written by vncli dotfiles: headroom for large source trees and builds.
fs.inotify.max_user_watches = 524288
fs.inotify.max_user_instances = 1024
fs.file-max = 2097152"
  run sudo sysctl --system
  log "Raised inotify watch and file-descriptor limits."
else
  warn "Skipped sysctl limits - requires sudo."
fi

# ---------------------------------------------------------------------------
# Windows controls with no Ubuntu counterpart
# Listed explicitly rather than silently dropped, so the two baselines can be
# read side by side.
# ---------------------------------------------------------------------------
log "No Ubuntu equivalent (nothing to disable): TIPC keystroke upload; ink/text/contact"
log "  harvesting; Accept-Language opt-out; Explorer file-extension hiding; Defender"
log "  cloud protection; Windows activation/licensing checks."

if [[ $DRY_RUN -eq 1 ]]; then
  echo ""
  log "Dry run complete - nothing was changed."
  if [[ $SUDO_SIMULATED -eq 1 ]]; then
    warn "Root-level changes were shown but sudo was not actually available."
  fi
elif [[ $HAS_SESSION -eq 1 && -f "$RESTORE_FILE" ]]; then
  echo ""
  log "Wrote gsettings restore script: $RESTORE_FILE"
fi

# vecnode 2026
