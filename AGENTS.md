# AGENTS.md — vncli `vn` CLI

Guidance for AI coding agents (and humans) working in this repository. `CLAUDE.md`
points here so both toolchains read the same instructions.

## What this is

`vncli` is a personal cross-platform CLI named **`vn`**, used to manage and run
day-to-day functions on the author's Windows 11 and Ubuntu 22.04 machines. The core
is a Rust binary that exposes:

- A **TUI dashboard** (the default when `vn` runs with no subcommand) for picking and
  running tasks interactively.
- **Subcommands** (`vn sys`, `vn docker`, `vn git`, `vn net`, `vn run`, `vn ai`, …)
  that the TUI shells out to, or that you can run directly.
- A **Windows system-tray agent** (`vn tray`) that stays resident as a small icon and
  spawns new elevated TUI terminals on demand.

Genuinely OS-specific work (dependency installs, ollama setup, dotfiles, starting
Docker Desktop) is delegated to per-OS scripts under `scripts/ubuntu22/*.sh` and
`scripts/win11/*.bat`, which `vn run` locates and executes. Everything cross-platform —
port scanning (`vn net`), and all Docker app launching/maintenance (`vn app`,
`vn docker`) — is implemented natively in Rust so there is one code path, not two.

## How it runs (the resident-tray model)

On Windows the intended flow is:

1. Double-click **`run_cli.bat`** — **no administrator rights required**. It unblocks the
   scripts, builds `vn`, installs `rustscan` if missing, then launches a **tray agent**
   (`vn tray`, minimized/hidden) and a first (non-elevated) TUI.
2. The tray agent runs persistently as a shield icon. Its menu has **"Open TUI Terminal"**
   (normal, same privilege as the tray — the default), **"Open Admin TUI Terminal"**
   (re-launches `vn` elevated via `ShellExecuteW "runas"`, triggering a UAC prompt), and
   **"Quit"**.
3. You open as many TUIs as you want from the tray; closing them leaves the tray
   running. Quitting from the tray exits everything.

Run everyday tasks from a normal (non-admin) TUI. Only use **"Open Admin TUI Terminal"**
for commands that actually need elevation (e.g. some Docker / doc-processor workflows);
that way `run_cli.bat` never has to be launched as Administrator.

A single-instance lock (`%TEMP%\vncli.vn.tray.lock`) keeps only one tray alive.
Linux has no tray; use `./run_cli.sh` to build and launch the TUI directly.

## Layout

```
run_cli.bat / run_cli.sh   Build + launch (Windows installs the tray; both auto-install rustscan)
cli/                       Cargo workspace
  Cargo.toml               Workspace + shared dependency versions
  crates/vn/
    Cargo.toml             The vn binary; tray-item + windows-sys are Windows-only deps
    src/
      main.rs              clap CLI definition + dispatch
      config.rs            TOML config at <config_dir>/vn/config.toml (ollama, sessions, prompts)
      tray.rs              Windows tray agent (cfg-gated; bails on non-Windows)
      tui/app.rs           The whole TUI: menu tree, process spawning, log/docker panels
      commands/
        mod.rs             Module list
        ai.rs              vn ai status|models|pull|chat (Ollama via the ollama-rs crate)
        apps.rs            vn app open|stop|list — native Docker app engine (see below)
        ollama/session.rs  Chat session persistence (per-session message history)
        sys.rs             vn sys info|update|clean (uses sysinfo)
        docker.rs          vn docker ps|up|down|prune|check|stop-all|remove-*
        git.rs             vn git sync|status
        net.rs             vn net scan — RustScan-based open-port scan (native Rust)
        run.rs             vn run <name> — maps script names to scripts/** and executes
        mcp.rs             vn mcp serve [--http] — MCP server entrypoint (see below)
      mcp/                 MCP (Model Context Protocol) host: see "MCP host" below
        mod.rs             Module list
        approval.rs        ApprovalGate — the confirm-before-destructive-tool-call bridge
        apps_toolset.rs     AppsToolset — the "Apps" toolset (list/open/stop apps)
scripts/ubuntu22/*.sh      Linux task scripts
scripts/win11/*.bat        Windows task scripts
scripts/tools-cli/alpine/  In-container tools workflow
dotfiles/win11/            Windows dotfiles: setup_dotfiles.bat + global_configs.ps1
dotfiles/ubuntu/           Ubuntu 22.04/24.04 dotfiles: setup_dotfiles.sh + global_configs.sh
docs/, docker/, assets/
```

## Dotfiles

`dotfiles/win11` and `dotfiles/ubuntu` are matched baselines — the same set of controls
(privacy/telemetry, package removal, power, screensaver, black background) expressed per
OS, plus each side's N/A list so the two can be read against each other. One Ubuntu script
covers 22.04 and 24.04: version differences (GNOME 42 vs 46 schemas,
`ubuntu-advantage-tools` vs `ubuntu-pro-client` unit names) are absorbed by probing whether
a key or unit exists, not by branching on `VERSION_ID`.

**The Linux side does not elevate itself, and must never prompt for a sudo password.**
`setup_dotfiles.bat` requests UAC once and runs everything as Administrator; that model
can't work on Linux, where `gsettings`/`dconf` need the *user's* session bus and
`systemctl`/`apt`/`sysctl` need root. `global_configs.sh` therefore runs unprivileged and
guards system-level sections behind a `sudo -n true` probe (`$HAVE_SUDO`), the same way
`global_configs.ps1` guards HKLM writes behind `$isAdmin`. The probe must stay
non-interactive: `vn run` spawns these with piped stdout/stderr, so a real sudo prompt
would hang the TUI's CLI Output panel with nothing visible to type into. If you add a
root-level section, gate it on `$HAVE_SUDO` and print a `[WARNING] Skipped …` line rather
than calling bare `sudo`. Desktop sections are likewise gated on `$HAS_SESSION`, and their
summary lines use `log_desktop` (silent without a session bus) so a headless run doesn't
claim to have applied settings that `gset` skipped.

`gset` records each key's outgoing value into a runnable restore script under
`~/.local/state/vncli/`; that covers dconf only, not package removals or `/etc` drop-ins.
`--dry-run` prints every change and applies none — use it to check a new section.

## Build, run, check

```bash
# Build
cargo build --manifest-path cli/Cargo.toml -p vn

# Run the TUI directly (no launcher)
cargo run --manifest-path cli/Cargo.toml -p vn --

# Run a subcommand
cargo run --manifest-path cli/Cargo.toml -p vn -- sys info
cargo run --manifest-path cli/Cargo.toml -p vn -- net scan        # local /24 open ports
cargo run --manifest-path cli/Cargo.toml -p vn -- net scan 10.0.0.5

# Lint / format before finishing a change
cargo clippy --manifest-path cli/Cargo.toml -p vn
cargo fmt --manifest-path cli/Cargo.toml
```

**`run_cli.bat` does not build into `cli/target/debug/`.** It detects the host triple
(`rustc -vV`) and runs `cargo build --target "%RUST_HOST%"`, producing a *separate* binary
at `cli/target/<host-triple>/debug/vn.exe` - a plain `cargo build` (no `--target`) writes to
`cli/target/debug/vn.exe` instead, a different file with its own independent build cache.
Rebuilding/testing against one does not update or verify the other, and cargo won't warn you
- it just reports the untouched one as already up to date. If you're validating a fix meant
to land in what the user's launcher actually runs, rebuild with the matching `--target`
(`rustc -vV | findstr host` for the triple) or just re-run `run_cli.bat` itself, not a bare
`cargo build`.

There is no test suite yet. **Always `cargo build` (and ideally `clippy`) after Rust
changes**; the TUI is hard to exercise headlessly. CI
([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs `cargo fmt --check`, `clippy
-D warnings`, and `cargo build --locked` on Ubuntu and Windows on every push/PR to
`main`, plus `cargo deny` and `cargo audit` — but it doesn't replace running these
locally before you push, since the TUI itself still needs a manual smoke test.

## How the TUI dispatches work

`tui/app.rs` defines a static menu tree (`MenuKind` + `menu_items`). Each leaf is a
`CommandItem` whose `Action::Execute(args)` re-invokes the **current `vn` executable**
as a child process with those args (e.g. `["run", "win11-check-internet"]` or
`["net", "scan"]`), piping stdout/stderr into the "CLI Output" panel. So adding a TUI
action = add a `CommandItem` pointing at an existing `vn` subcommand. Spawned commands
run via the `command-group` crate (`group_spawn()`), so each owns its own process
group/job object — killing one (or quitting the TUI) also kills anything it spawned
(e.g. a `docker build`), instead of leaving it orphaned.

Destructive menu items use `Action::ExecuteConfirm` instead of `Action::Execute`: it
arms the input box with `InputPurpose::ConfirmDestructive` and only runs the command if
you type `yes`. Use it for anything that stops/removes containers or images; regular
commands stay `Action::Execute`.

Keybindings beyond Up/Down/Enter: `x` kills every running background process without
quitting the TUI; `e` scrolls the CLI Output panel to the previous error/stderr line;
`Esc` steps back one menu level (or cancels the input box), only quitting from the root
Dashboard — `q` still quits immediately from anywhere in the Dashboard. Mouse wheel
scrolls CLI Output; left-clicking a dashboard row selects and runs it. `**bold**` markers
in CLI Output text (common in AI replies) render bold via `spans_with_bold`.

**Every line from an MCP tool call gets its own `LogEntry::Mcp` variant**, not `Info` -
`ProcEvent::McpActivity` events (both the `[MCP] Calling .../[MCP] Result: ...` tag lines and
the untagged continuation lines `split_to_entries` breaks a multi-line result, e.g. a table,
into) all render Magenta by default (`tagged_line_color(text, Color::Magenta)`), overridden
only by a line's own more specific tag (a live-streamed docker build's `[DOCKER]` lines stay
Blue). Previously these were plain `Info` entries, so only the one line that happened to
literally start with `[MCP]` was colored - every other line from that same call (a table's
data rows, say) was indistinguishable grey with no visual link back to which call produced
it. MCP tools that return tabular text (`docker_list_containers`, `system_list_processes`)
build it with `mcp/report.rs`'s `format_table` (space-padded fixed-width columns), not raw
`\t` - a real terminal expands tabs to stops, but ratatui's `Paragraph` doesn't, so a
tab-separated table misaligned badly once word-wrap was involved, occasionally badly enough
to look like text from a different line overlapping it.

**CLI Output scroll is in wrapped *rows*, not log *entries*.** The `Paragraph` is rendered
with `Wrap { trim: false }`, and ratatui's own wrap calculation treats `.scroll()`'s offset
as a rendered-row count - a single long line (a docker build command, easily 300+ characters)
wraps into several rows. `AppState::output_total_rows` (via ratatui's
`Paragraph::line_count`, gated behind the `unstable-rendered-line-info` cargo feature - not
an approximation, the exact same wrap calculation the render call itself uses) is what
`max_output_scroll`/`clamp_output_scroll` need to compute "scroll to the true bottom" against
- using `self.logs.len()` (entry count) there was a real bug: following new output would
visibly stop short of the actual tail whenever enough wrapped lines had accumulated above the
current position, looking like the panel had "frozen" until an unrelated scroll/resize
happened to close the gap. `jump_to_previous_error` still treats `output_scroll` as an entry
index into `self.logs` (a known, documented approximation - only affects how far back one
keypress jumps, not whether new output is visible).

**Report-based tool calls no longer double-log their own output.** A tool built on
`run_reported`/`LiveReporter` (an `open_app` docker build, `docker_stop_all`, etc.) used to
print every line twice: once live as it streamed, then again in full inside the final
`[MCP] Result: ...` message (`call_tool_result_text` returns the same joined text regardless
of whether it was already streamed). `spawn_chat_worker` now tracks whether `tool_live` was
invoked at all during a given call (`live_line_count`, reset before each call) and prints a
short "(see the streamed output above)" placeholder instead of the full text when it was -
the *full* text is still what's fed back to the model via `ChatMessage::tool(text)`
regardless; only the CLI Output/log line is shortened.

**Nothing that can block should run on the render loop.** Spawned commands, the Ollama
chat worker, and the embedded MCP server each own a dedicated OS thread (or tokio runtime)
and talk back to `event_loop` only through channels drained non-blockingly
(`pump_process`/`pump_mcp_approvals`/`pump_docker_panel`); `docker ps` for the Docker panel
follows the same pattern (`refresh_docker_panel` spawns a thread, `pump_docker_panel`
applies the result when it arrives) so a slow/hung Docker daemon can't stall the whole
TUI. If you add a new background action, follow this pattern rather than calling a
blocking operation directly from `event_loop`.

`commands/run.rs` `map_script()` is a match from a script name to a relative path
under `scripts/`. To add a script-backed task: drop the script in the right OS folder,
add a `map_script` arm, and wire a `CommandItem` for it. Menu items are OS-gated by
`menu_allowed_on_current_os`. Docker-app names are intercepted first by `try_native()`
and routed to `commands/apps.rs` — scripts are only for genuinely OS-specific work
(package installs, ollama, dotfiles, Docker Desktop startup).

Repo root is resolved via `--repo-root`, `VNCLI_REPO_ROOT`, the config location, or
by walking up for a dir that has both `.git/` and `scripts/`.

## Networking: open-port scanning

`vn net scan [target]` ([cli/crates/vn/src/commands/net.rs](cli/crates/vn/src/commands/net.rs))
runs [RustScan](https://github.com/bee-san/RustScan) — `rustscan -a <target> --greppable`.
With no target it derives the host's local `/24` (via a non-sending UDP-connect trick)
and scans the whole subnet, which is far faster than the old ping-sweep scanner it
replaced. `--greppable` keeps it self-contained (no nmap hand-off required). RustScan is
a standalone binary, **not** a linkable crate; the launchers and `check_dependencies`
scripts install it with `cargo install rustscan`.

## AI / Ollama

`vn ai` talks to a local [Ollama](https://ollama.com) server through the
[`ollama-rs`](https://crates.io/crates/ollama-rs) crate (a normal Cargo dependency, so
it is compiled in automatically — no separate install like RustScan). Subcommands:

- `vn ai status` — is the Ollama server reachable?
- `vn ai models [--tools-only]` — list installed model names (one per line; used by the
  TUI). `--tools-only` checks each model's reported `capabilities` via `ollama show` and
  skips ones without `"tools"` (see `model_supports_tools`) — the TUI's chat always
  attaches tools (see below), so a model without that capability can't chat at all.
- `vn ai pull <name>` — download a model (e.g. `llama3.2`, which is small and supports
  tools).
- `vn ai chat "<message>" [--model m] [--session s]` — send a message; context is kept
  per session via [ollama/session.rs](cli/crates/vn/src/ollama/session.rs) so turns
  build on each other across invocations.

In the TUI's AI submenu (win11-ai / ubuntu22-ai): **Select Model** opens a dynamic menu
listing installed **tool-capable** models only (the TUI shells out to `vn ai models
--tools-only` and builds rows from the output; the chosen model shows in the header and
is passed as `--model`). If none are installed, the menu shows a placeholder recommending
`vn ai pull llama3.2`. **Download Model** and **Chat** arm the input box (see
`InputPurpose`) so the next typed line is
routed to `vn ai pull` / `vn ai chat`. Output streams into the CLI Output panel and the
session log file like any other command. The Ollama *server* itself still needs to be
installed/running — that is what the `check-ollama` / `open-ollama` scripts handle.

## Dockerized apps (Open menu) — native `vn app`

The TUI **Open** submenu launches/controls self-hosted web apps in Docker through **one
cross-platform Rust code path**: [cli/crates/vn/src/commands/apps.rs](cli/crates/vn/src/commands/apps.rs).
There are **no per-OS launch scripts anymore** — the old `run_*/stop_*` `.bat`/`.sh` pairs
were replaced by:

- `vn app open <name> [--no-open]` — check Docker, build (local apps) or pull the image,
  create/start the container, wait for the port, open **Chrome** (default-browser fallback).
- `vn app stop <name>` — stop the container (kept for fast reopen where applicable).
- `vn app list` — the app registry.
- `vn docker check|stop-all|remove-containers|remove-images` — global maintenance.

Legacy names still work: `vn run win11-open-library` etc. route to the native
implementation (see `try_native` in `run.rs`), so old muscle memory and docs don't break.

Every app is an `AppPlan` in `plan_for()` — image, container, optional build context,
lifecycle (`Recreate` vs `Reuse` running/stopped), ports, env, mounts, readiness port.
**To add an app: add one `plan_for` arm and a `CommandItem` in both Open submenus.**
The engine enforces the security posture automatically (don't bypass it): ports published
**loopback-only** (`127.0.0.1`), `--cap-drop ALL`, `--security-opt no-new-privileges`,
`--pids-limit` for locally built images, and on Linux `--user $(id -u):$(id -g)` where the
plan sets `linux_user` (so bind-mount files stay user-owned). See SECURITY.md.

Apps: BentoPDF (`ghcr.io/alam00000/bentopdf-simple`, port 8080, AGPL-3.0 self-hosted build,
reuses its container), translator (`libretranslate/libretranslate`, port 5000, AGPL-3.0,
reuses its container - first run downloads every language's translation model, ~9-10 GB
and 5-10 minutes), docs
(mdBook, port 3000), plus the locally built **library** (8090, image source
`docker/library-portal/`), **doc-processor** (8085/8086, image source
`docker/media-processor/`) and **media-downloader** (8095), which rebuild + recreate on
every open (picks up code edits).

**media-downloader (custom, locally built):** a tiny yt-dlp + ffmpeg web app in
[docker/media-downloader/](docker/media-downloader/) — `debian:12-slim` + a single stdlib
`app.py`. `yt-dlp` is installed via `pip` (the Debian apt package is years stale and breaks
against current sites). Paste a video URL, pick **MP3 / WAV / MP4**; the file is saved to the
host **Desktop**, bind-mounted at `/output` by `vn app open media-downloader` (port 8095). Because it fetches arbitrary web links, it is hardened: the container runs **non-root**
(Linux uses `--user $(id -u):$(id -g)`), with **`--cap-drop ALL`**, **`--security-opt
no-new-privileges`** and a pids limit; the app accepts only http/https URLs and rejects
hosts that resolve to loopback/private/link-local up front. Beyond that initial check,
`yt-dlp` itself is pointed at a small **in-container egress-guard proxy**
(`start_egress_proxy` in `app.py`, `--proxy http://127.0.0.1:$EGRESS_PROXY_PORT`) that
re-resolves and re-validates every connection yt-dlp makes at actual connect time — this
is what catches a redirect to a different (private) host or DNS rebinding between the
initial check and yt-dlp's own lookup, since the fast-fail check alone only ever sees the
first URL. yt-dlp also runs with `--ignore-config --restrict-filenames --max-filesize`,
and saves via a sanitized, traversal-checked, collision-safe filename confined to the
mount.

**library (custom, locally built):** a lightweight viewer/manager for the repo's
`library/` folder, living in [docker/library-portal/](docker/library-portal/) —
`python:3.12-slim` + a single stdlib `app.py`, plus PyMuPDF for thumbnails.
`vn app open library` builds the image (the build context is only
`docker/library-portal/`, so **no PDFs enter the image**), then runs it with the repo
`library/` bind-mounted on port 8090 and opens Chrome. The server walks `/library` per
request and renders an index in the same light, simple card style as doc-processor's UI
(shared `--bg`/`--surface`/`--accent` tokens, no serif headings), streaming PDFs inline for
the browser viewer.
It supports edit/rename, per-document tags, delete, list/grid/tree views, sort, and (in tree
view) creating folders and drag-and-drop moving PDFs into them. App state
(metadata overrides + tags) is kept in `library/.portal/portal.json` and thumbnails are
cached under `library/.portal/thumbs/` (both gitignored, hidden from the listing); PDFs are
only modified on an explicit rename. `open` rebuilds + recreates the container each time
(picks up `app.py` edits).

Note on `.bat`: inside an `if (…) else (…)` block, any literal `(`/`)` in an `echo`
must be escaped as `^(`/`^)` (or avoided) — an unescaped `)` closes the block early and
cmd fails with "… was unexpected at this time."

## MCP host

vncli is an MCP ([Model Context Protocol](https://modelcontextprotocol.io)) **host**: it
exposes its own host-control functions as MCP tools, using the official
[`rmcp`](https://crates.io/crates/rmcp) Rust SDK. Three toolsets so far, all implemented as
`#[tool_router]` blocks on the same `AppsToolset` struct (rmcp merges same-typed routers
with `+`; see `AppsToolset::new`) — there is exactly one implementation whether the caller
is an external MCP client or vncli's own Ollama chat:

- **Apps** ([cli/crates/vn/src/mcp/apps_toolset.rs](cli/crates/vn/src/mcp/apps_toolset.rs)):
  `list_apps`, `open_app`, `stop_app`, `restart_app` — thin wrappers around
  `commands::apps::{list, open_reported, stop_reported}` (`restart_app` composes
  `stop_reported` + `open_reported` in one gated call).
- **Docker** ([cli/crates/vn/src/mcp/docker_toolset.rs](cli/crates/vn/src/mcp/docker_toolset.rs)):
  `docker_list_containers`, `docker_container_logs`, `docker_check`, `docker_disk_usage`
  (read-only — the last wraps `docker system df`) and `docker_stop_all`/
  `docker_remove_containers`/`docker_remove_images` (host-wide and hard/impossible to undo,
  so gated) — introspection into and maintenance of whatever docker actually has (any
  container/image, not just vncli's own apps), via `commands::apps::{docker_ps_all,
  docker_logs_tail, docker_check_reported, docker_stop_all_reported,
  docker_remove_containers_reported, docker_remove_images_reported, docker_disk_usage}`.
  Every tool name here is prefixed `docker_`, even the read-only ones, since all toolsets
  share one flat `tools/list` namespace with `list_apps`/`open_app`/etc - the prefix is what
  tells an LLM a name means "docker-wide" rather than "one vncli app".
  `docker_container_logs` also takes an optional `pattern` (regex, via the `regex` crate):
  when set, it scans the last `SEARCH_SCAN_LINES` (5000) lines instead of the plain
  `lines`-bounded tail and returns only matching lines, capped at `MAX_MATCHES` (200) - see
  `search_or_tail_logs` in `docker_toolset.rs`.
- **System** ([cli/crates/vn/src/mcp/system_toolset.rs](cli/crates/vn/src/mcp/system_toolset.rs)):
  `system_list_processes` - lists every OS process on the host (not docker containers, not
  vncli apps): PID, name, exe path, parent PID, status, memory, uptime; optional `filter`
  substring. Read-only, no gate. Queried live via [`sysinfo`](https://crates.io/crates/sysinfo)
  (already a dependency - see `commands::sys`/`tray.rs`) rather than shelling out to
  `tasklist`/`ps`, so each call is a fresh, uncached snapshot with no subprocess-spawn
  overhead - "real-time" here means every call reflects current state, not a continuous push
  (MCP's `tools/call` is request/response, so there's no live-streaming tool-call mode).
  Unfiltered results are capped at 300 rows on a busy host.

**Report-based tools** (every tool above except `list_apps`,
`docker_list_containers`/`docker_container_logs`/`docker_disk_usage`, and
`system_list_processes`, which just fetch and return text) share plumbing in
[mcp/report.rs](cli/crates/vn/src/mcp/report.rs): `run_reported` runs an `apps::*_reported`
fn on a blocking task and joins its captured lines into the `CallToolResult`, and
`require_approval` produces the standard denial result for gated tools. Each such tool is
split into a thin `#[tool]` wrapper and an `_impl(params, live: Option<LiveReporter>)`
method - the `#[tool]` macro's wire dispatch can't carry an extra parameter, so the wrapper
calls `_impl(params, None)`, while `call_by_name` (the in-process Ollama chat path) calls it
with `Some(live)`. `LiveReporter` is a `Fn(&str)` callback: the TUI's chat worker
(`spawn_chat_worker`) supplies one that forwards each line straight into the CLI Output panel
as it's produced (via `ProcEvent::McpActivity`), instead of the panel sitting idle for the
whole tool call and then dumping every line at once when it returns - which is what a slow
`open_app` docker build or `docker_remove_images` used to look like before this existed.
External MCP clients never get a `LiveReporter`: MCP's `tools/call` is request/response, so
they only ever see the final joined result regardless.

A full list of tools and what they do is tracked for humans in
[docs/mcp-functions.md](docs/mcp-functions.md) — update it whenever a tool is added,
removed, or its behavior changes.

**`open_app`/`restart_app`'s browser-opening default** depends on who's calling, via
`AppsToolset::new`'s `default_no_open` param (used when the tool call omits `no_open`):
headless/external callers (`vn mcp serve`, and the TUI's embedded server for external
clients) pass `true` — an LLM with no one watching popping your browser is a surprising
side effect. The in-TUI Ollama chat (`spawn_chat_worker`) passes `false` — the user is
present and asked for it, so the browser actually launches once the app is ready (Chrome if
installed, else the OS default — see `commands::apps::open_browser`).

**Reaching it:**
- `vn mcp serve` — stdio transport, for MCP clients that spawn their own subprocess
  (Claude Desktop/Code's usual local-server config). Runs headless: no TUI is attached.
- `vn mcp serve --http [--port 7332]` — loopback-only Streamable HTTP transport, same
  headless behavior.
- **The TUI's embedded server**: on startup, `tui/app.rs` spawns the same HTTP transport
  on its own thread (`spawn_mcp_server`), bound to `127.0.0.1:7332` by default. The "MCP
  Server" panel shows its status and tool count. This is the one that gets a real approval
  prompt (see below), since the TUI is attached.

**Approval gate** ([cli/crates/vn/src/mcp/approval.rs](cli/crates/vn/src/mcp/approval.rs)):
destructive tools (`stop_app`, `restart_app`, `docker_stop_all`,
`docker_remove_containers`, `docker_remove_images`) call `ApprovalGate::request()` (via
`AppsToolset::approval()`, a `pub(crate)` accessor since the docker toolset lives in a
different module) before acting. With the TUI attached, this arms the input box with the
same "type yes to confirm" pattern used for destructive menu items
(`InputPurpose::ApproveMcp`) and blocks the tool call until you answer. Headless (`vn mcp
serve`), `ApprovalGate::headless()` auto-denies every request — fail-closed, since there's
no console free to prompt on (stdio *is* the protocol channel). Everything else (`list_apps`,
`open_app`, `docker_list_containers`, `docker_container_logs`, `docker_check`,
`docker_disk_usage`, `system_list_processes`) isn't gated; they run immediately — either
read-only, or (for `open_app`) matching the TUI's own "vn app open" menu items.
`docker_stop_all`/`docker_remove_containers`/`docker_remove_images` are gated even though
there's no single-app equivalent requiring it, because unlike `stop_app` they act on every
container/image on the host, not just vncli's own.

**Ollama chat integration** (`spawn_chat_worker` in
[tui/app.rs](cli/crates/vn/src/tui/app.rs)): every chat turn attaches `ToolInfo` entries
built dynamically from `AppsToolset::list_tools()` (the same JSON-schema metadata an MCP
client sees via `tools/list` — `ollama-rs`'s `ToolInfo`/`ToolFunctionInfo` are plain public
structs, so no compile-time-known tool types are needed). If the reply has `tool_calls`,
each one is dispatched via `AppsToolset::call_by_name` (in-process, no HTTP/stdio
round-trip — `stop_app` still goes through the same `ApprovalGate`), logged into the CLI
Output panel as `[MCP] Calling ...` (report-based tools also stream their own progress lines
live in between, see `LiveReporter` above) / `[MCP] Result: ...`, and fed back as a
`ChatMessage::tool(...)` — capped at `MAX_TOOL_ROUNDS` (4) to bound a confused model's
tool-calling loop. Every `ollama.send_chat_messages` call is wrapped in
`tokio::time::timeout(CHAT_REQUEST_TIMEOUT, ...)` (5 minutes) - `ollama-rs`'s HTTP client has
no timeout of its own, so a hung/stuck local model (e.g. one confused by a hallucinated tool
call into generating a runaway reply) would otherwise block the single chat-worker thread
forever with no error and no crash, which looks like "the CLI Output panel just stopped"
rather than an obvious hang, since the rest of the TUI runs on other threads and stays
responsive.

**Nudging the model to actually call tools:** confirmed by hand-crafting the exact
`/api/chat` request and replaying it directly against Ollama (bypassing this codebase
entirely) - even a model fine-tuned for tool use (`llama3-groq-tool-use`) reliably *stops*
calling tools for a state question ("list the containers") once even one prior assistant
turn with no tool call is already in its context; repeated runs of the identical multi-turn
request went from 0/3 to 5/5 tool calls with two changes: (1) `CHAT_TEMPERATURE` (0.2,
lower than a model's usual ~0.7-0.8 default) cuts run-to-run variance, and (2)
`TOOL_USE_REMINDER` is appended to the *outgoing* copy of the latest user message only (not
to what's displayed or saved to the session - `req_message` stays exactly what the user
typed) telling the model to answer by calling a tool rather than from memory. Ollama has no
`tool_choice: required` equivalent to force this outright (checked against a running 0.31.1
server) - this is a mitigation, not a guarantee. It was validated against a short (1-2 turn)
history; a real session's persisted history has no automatic expiry and was observed to reach
108 messages after a day of use, which the reminder alone isn't strong enough to overcome -
see `MAX_HISTORY_MESSAGES` below, which addresses that directly. (Also: this mitigation only
takes effect once you're running a binary actually built *after* the fix landed - see the
`run_cli.bat` build-target note above. Verifying against a plain `cargo build`'s output while
`run_cli.bat` launches a separately-cached `--target`-specific binary is exactly how this got
missed the first time.)

**Bounding how much history reaches the model:** `MAX_HISTORY_MESSAGES` (12) caps how many of
`session.messages` are replayed into a chat request, independent of `MAX_LOG_ENTRIES` (which
only bounds the CLI Output display) and independent of the session file itself (which keeps
growing - `session.messages.clear()` only ever runs on `ChatRequest::ResetSession`, i.e. an
in-run model switch, not automatically). The more of a long-lived session's history consists
of tool-call-free replies, the more it anchors the model away from calling tools at all
(regardless of `TOOL_USE_REMINDER`) - trimming what's *sent* keeps that bounded without
touching what's *saved*.

**Ground truth for hallucinated tool use:** a small local model will sometimes claim to have
done something ("restarted the container!") in its final reply without actually calling any
tool that turn - `spawn_chat_worker` tracks whether at least one tool call was dispatched
during the turn and, if not, appends `MCP_NONE_TAG` (`[MCP: NONE]`) on its own line *after*
the reply (`"{reply}\n{MCP_NONE_TAG}"`) *before* it's saved into the session (so future turns'
history, and `default_system_prompt()`'s instructions, can see which of the model's own past
replies weren't grounded) and shown. Deliberately not `[MCP]` alone: that would collide with
the existing `[MCP] Calling .../[MCP] Result: ...` activity-line prefixes when grepping a log
for one or the other. `pump_process`'s `split_to_entries` turns that `\n` into its own log
line, and the render loop (`spans_for_stdout_line` in
[tui/app.rs](cli/crates/vn/src/tui/app.rs)) renders a line that's exactly the tag entirely in
the same Magenta as `[MCP]` activity lines, rather than the reply's normal DIM tone - on its
own line and separately colored so it reads as a distinct marker, not part of the message.

**`OLLAMA_MARKER` (`[OLLAMA]`)** wraps every chat reply, one on its own line before and one
after (`"{OLLAMA_MARKER}\n{model}: {reply}\n{OLLAMA_MARKER}"`), purely so the reply's
start/end are visually obvious once other output (MCP activity, docker build lines) has
scrolled by in between turns. Unlike `MCP_NONE_TAG`, this is display-only - added *after*
`session.append_assistant` so it's never saved into history or fed back to the model, since
it carries no information worth the model seeing. Renders in the same DIM grey as the reply
body with no special-casing needed: it just doesn't match `[MCP]`/`[DOCKER]` in
`tagged_line_color`, so it falls through to that function's default.

**Session reset on model switch:** the persisted "tui" session (one file, loaded once when
`spawn_chat_worker` starts) used to survive switching the active model in the "Select Model"
menu - a newly-selected model would inherit whatever the *previous* model said, including
any fabricated replies, and tends to treat that prior turn as established fact rather than
independently re-checking it (observed directly: two different models produced byte-identical
fabricated container IDs because the second one was conditioning on the first one's
fabrication in shared history). `activate_selected` now sends `ChatRequest::ResetSession`
through `chat_tx` whenever the newly-picked model differs from the previously-selected one
*within this run* (not on the first selection after startup, which would otherwise needlessly
discard a legitimate resumed conversation with the same model from a previous run) -
`spawn_chat_worker` clears `session.messages` and persists the empty session.

**`default_system_prompt()`** ([config.rs](cli/crates/vn/src/config.rs)) explicitly instructs
the model to call a tool for any state question rather than guess, to re-call rather than
reuse a previous answer since state can change between turns, and to treat a
`[MCP: NONE]`-tagged prior reply as unverified. Only applied to a *freshly generated*
`config.toml` - `load_or_init` never overwrites an existing one, so an existing install needs
its `prompts.system` value hand-edited (or the file deleted to regenerate) to pick this up;
the config lives at `dirs::config_dir()/vn/config.toml` (`default_config_path`).

**To add a tool:** add a `#[tool(description = "...")]` method (with a params struct
deriving `serde::Deserialize` + `schemars::JsonSchema` if it takes arguments) to an existing
`impl AppsToolset` toolset block, and add a matching arm in `call_by_name` (or the relevant
`call_*_by_name` fallback, like `call_docker_tool_by_name`) so the Ollama integration can
dispatch to it by name. If it reports progress rather than completing near-instantly, build
it on `mcp/report.rs`'s `run_reported`/`require_approval` and split it into a thin `#[tool]`
wrapper plus an `_impl(params, live: Option<LiveReporter>)` method (see `open_app`/
`open_app_impl`) so it streams to the TUI's chat like the others. **To add a whole new
toolset:** a new file under `mcp/` with its own
`impl AppsToolset { #[tool_router(router = my_router)] ... }` block (see
`docker_toolset.rs`), merged into `AppsToolset::new`'s `tool_router` field with `+` and
dispatched via a new `call_*_by_name` fallback from `call_by_name`. Bump the tool count
(`docker_toolset::TOOL_COUNT` is added into `apps_toolset::TOOL_COUNT`, which feeds the
TUI's "MCP Server" panel), and add the tool to
[docs/mcp-functions.md](docs/mcp-functions.md).

## Conventions

- Match the surrounding style: `anyhow::Result` with `.context(...)`, `cfg!`/`#[cfg]`
  gating for OS-specific code, no panics in command paths.
- Keep cross-platform behavior in mind — almost everything must work on both Windows
  and Linux. Windows-only code (tray, windows-sys) is `#[cfg(target_os = "windows")]`.
- Scripts come in matched `.sh` (ubuntu22) / `.bat` (win11) pairs; when you add or
  change one, consider the other.
- Don't commit `cli/target/` (gitignored). Commit/push only when asked.
