Ubuntu 22.04 / 24.04 dotfiles

The Linux counterpart of [`dotfiles/win11`](../win11/README.md). One script covers both
LTS releases: version-specific differences (GNOME 42 vs 46 schemas,
`ubuntu-advantage-tools` vs `ubuntu-pro-client` unit names) are absorbed by checking
whether a key or unit actually exists, not by branching on `VERSION_ID`.

Run it as your **normal desktop user** — not with `sudo`:

```bash
./setup_dotfiles.sh              # apply
./setup_dotfiles.sh --dry-run    # print every change, apply nothing
```

Or through the CLI: `vn run ubuntu22-setup-dotfiles` (also in the TUI under
`vn run ubuntu22` > `vn run ubuntu22-dotfiles`, where it asks you to type `yes` first).

## Two differences from the Windows script

**It does not elevate itself.** `setup_dotfiles.bat` requests UAC once and then does
everything as Administrator. That cannot work here: `gsettings`/`dconf` must run as the
desktop user against a live session bus, while `systemctl`/`apt`/`sysctl` need root. So
this runs unprivileged and guards its system-level sections behind a sudo check, exactly
the way `global_configs.ps1` guards its HKLM writes behind `$isAdmin`.

**It never prompts for a sudo password.** `vn run` spawns it with piped stdout/stderr, so
an interactive prompt would hang the TUI's output panel with nothing visible to type
into. The check is `sudo -n true`; if that fails, every root-level section is skipped with
a `[WARNING]`. To apply those sections, run `sudo -v` first (or run the script from a
plain terminal) and then re-run.

Sections that need a GNOME session are skipped the same way on a headless machine.

## What `setup_dotfiles.sh` does

- Refuse to run as root, so the sudo model above holds.
- Ensure the user SSH folder exists at `~/.ssh` (mode `0700`).
- Backup any existing SSH config to a randomized `mktemp` backup file.
- Copy `dotfiles/ubuntu/ssh/config` to `~/.ssh/config` (mode `0600`).
- Execute `global_configs.sh`, passing through `--dry-run`.

## What `global_configs.sh` applies

Grouped to match the Windows baseline it mirrors.

**Telemetry and reporting**

- Opt out of diagnostic telemetry by running `ubuntu-report -f send no`.
- Opt out of package statistics by setting `PARTICIPATE="no"` in `/etc/popularity-contest.conf` and disabling `popularity-contest.timer` when sudo is available.
- Disable crash collection and upload by disabling `whoopsie.service`, `whoopsie.path`, and `apport.service`, and setting `enabled=0` in `/etc/default/apport` when sudo is available.
- Disable the crash-report prompt by setting `com.ubuntu.update-notifier show-apport-crashes` to `false`.

**Phone-home and advertising**

This is the equivalent of *both* Windows license sections — the scheduled-task disable and
the `sppsvc` outbound firewall block. Ubuntu has no activation service to firewall off;
what it has is a set of timers that contact Canonical for Pro/ESM contract state and MOTD
advertising copy. Masking those units **is** the block — there is no per-process outbound
rule to add, and `ufw` cannot express one.

- Disable `apt-news.service`, `esm-cache.service`, `ua-timer.timer`, `ua-timer.service`, `ua-messaging.timer`, `ua-messaging.service`, `ua-reboot-cmds.service`, `ubuntu-advantage.service`, `motd-news.timer`, and `motd-news.service` when present and sudo is available.
- Disable login-banner ads by setting `ENABLED=0` in `/etc/default/motd-news`.
- Neutralize the apt Pro/ESM advertising hook by emptying `/etc/apt/apt.conf.d/20apt-esm-hook.conf`.
- Disable online search leakage by setting `org.gnome.desktop.search-providers disable-external` to `true` (covers both the Bing-in-search and Start-suggestions knobs).
- Disable store promotion surfaces by setting `org.gnome.software` `download-updates`, `download-updates-notify`, and `show-ratings` to `false`.

**Packages**

- Remove preinstalled apps listed in `BLOAT_PACKAGES` at the top of the script, and `apt-mark hold` each one so an upgrade cannot restore it. The default list is only GNOME games (`aisleriot`, `gnome-mahjongg`, `gnome-mines`, `gnome-sudoku`, `gnome-sushi`); `thunderbird`, `rhythmbox`, `libreoffice-core`, and `simple-scan` are listed commented out — uncomment what you actually want gone.
- Hold snap auto-refresh for 60 days by running `snap set system refresh.hold=<timestamp>`. `snapd` caps this at ~90 days, so it is renewable rather than permanent — re-run the script to extend it.
- Report configured GNOME Online Accounts rather than deleting them; removing them is left to Settings > Online Accounts, since deleting credentials is beyond what the Windows OneDrive section did.

**Desktop**

- Show hidden files by setting `org.gnome.nautilus.preferences show-hidden-files` and `org.gtk.Settings.FileChooser show-hidden` to `true`.
- Show the full path by setting `org.gnome.nautilus.preferences always-use-location-entry` to `true`.
- Restrict remote thumbnailing by setting `org.gnome.nautilus.preferences show-image-thumbnails` to `local-only`.
- Disable the screen reader by setting `org.gnome.desktop.a11y.applications screen-reader-enabled` to `false` and clearing the `screenreader` media key (the Super+Alt+S hotkey).
- Disable the screensaver by setting `org.gnome.desktop.screensaver idle-activation-enabled` and `lock-enabled` to `false`, and `org.gnome.desktop.lockdown disable-lock-screen` to `true`.
- Set a solid black desktop background by clearing `org.gnome.desktop.background` `picture-uri` and `picture-uri-dark` and setting `primary-color`/`secondary-color` to `#000000` with `color-shading-type` `solid`. No image file is generated — GNOME renders a solid colour natively and dconf applies it live, so the Windows `black.bmp` + `SystemParametersInfo` dance has no counterpart.

**Power**

- Never sleep by setting `org.gnome.settings-daemon.plugins.power` `sleep-inactive-ac-type` and `sleep-inactive-battery-type` to `nothing`, with both timeouts `0`.
- Never blank the screen by setting `org.gnome.desktop.session idle-delay` to `0` and `idle-dim` to `false`.
- Never suspend on lid close by writing `HandleLidSwitch=ignore` (plus the docked and external-power variants) to `/etc/systemd/logind.conf.d/99-vncli.conf`. Takes effect at next login.
- Disable hibernation by masking `hibernate.target` and `hybrid-sleep.target`. `suspend.target` is deliberately left alone, so manual sleep still works.

**Build headroom**

- Raise the ceilings that large source trees hit by writing `fs.inotify.max_user_watches`, `fs.inotify.max_user_instances`, and `fs.file-max` to `/etc/sysctl.d/99-vncli.conf` and applying with `sysctl --system`. This is the analog of the Windows `LongPathsEnabled` setting — not a path-length limit (Linux has none worth raising), but the same class of "the packaged default is too low for real builds".

## Undo

Before overwriting any `gsettings` key, the previous value is recorded into a runnable
restore script at `~/.local/state/vncli/dotfiles-restore-<timestamp>.sh`. Run it to put
the desktop settings back.

It covers **dconf only**. Package removals, masked systemd units, and files written under
`/etc` are not reverted by it — undo those with `apt install` / `apt-mark unhold`,
`systemctl unmask`, and deleting the `99-vncli.conf` drop-ins.

## Windows controls with no Ubuntu counterpart

Listed so the two baselines can be read side by side. The script prints these at the end
of a run rather than silently dropping them:

- TIPC keystroke collection and upload — no OS-level equivalent.
- Speech/inking/typing personalization and contact harvesting — no OS-level equivalent.
- `HttpAcceptLanguageOptOut` — Linux has no system Accept-Language API; this is browser-level only.
- Explorer file-extension hiding — Linux filenames always show in full.
- Defender cloud protection and sample submission — no bundled AV; the closest analog (crash-dump upload) is covered by the whoopsie/apport section.
- Windows activation and license validation — Ubuntu has nothing to activate.

## Note for 24.04

24.04 ships `kernel.apparmor_restrict_unprivileged_userns=1`, which blocks unprivileged
user namespaces. This is not touched by these dotfiles, but it is worth knowing about: it
breaks some Electron apps and container/sandbox workflows, including ones this repo
launches. Check with `sysctl kernel.apparmor_restrict_unprivileged_userns` if something
that works on 22.04 fails on 24.04 with a permission error.
