# vncli - Security Policy

**Last Updated:** July 2026
**Version:** 0.1.0

This document describes the security practices and threat model for vncli: the
`vn` Rust CLI/TUI, its per-OS scripts, and the Dockerized web apps launched from
the TUI's Open menu. It is written to match the code as it exists in this
repository.

---

## Table of Contents

1. [Rust CLI Security Practices](#rust-cli-security-practices)
2. [Script Execution Model](#script-execution-model)
3. [Dockerized Web Apps](#dockerized-web-apps)
4. [MCP Host](#mcp-host)
5. [Filesystem Access](#filesystem-access)
6. [Network Access](#network-access)
7. [Additional Resources](#additional-resources)

---

## Rust CLI Security Practices

### Unsafe Code

The cross-platform command/TUI code (`main.rs`, `config.rs`, `commands/`, `tui/`) is
100% safe Rust. The **Windows-only tray module** (`tray.rs`, `cfg(target_os =
"windows")`) contains a small number of reviewed `unsafe` blocks around `windows-sys`
FFI calls (`ShellExecuteW`, `LoadIconW`, `MessageBoxW`, `GetConsoleWindow`) needed to
drive the system tray, show the UAC-elevated terminal, and hide the console window —
there is no safe-Rust equivalent for these Win32 calls. It never runs on Linux and is
excluded from the rest of the crate.

**Verification:**
```bash
grep -rl "unsafe" cli/crates/vn/src/
# Returns: cli/crates/vn/src/tray.rs (Windows-only FFI, see above)
```

### Process Lifecycle

Commands the TUI spawns in the background (`vn app open`, `vn run ...`, `vn ai pull`,
etc.) run via the [`command-group`](https://crates.io/crates/command-group) crate
instead of `std::process::Command::spawn()` directly, so each spawned command owns its
own process group (Unix) / job object (Windows). Quitting the TUI or killing a running
job also kills anything that command itself spawned (e.g. a `docker build` or
`yt-dlp` invocation) instead of leaving it orphaned in the background.

### Dependency Auditing

- **`cargo audit`** scans `Cargo.lock` against the RustSec Advisory Database.
- **`cargo deny`** (see [cli/deny.toml](cli/deny.toml)) warns on unmaintained
  crates and non-permissive licenses, and bans `openssl` in favor of rustls.
- Both now run in CI on every push/PR to `main`
  ([.github/workflows/ci.yml](.github/workflows/ci.yml)), alongside `cargo fmt --check`,
  `cargo clippy -D warnings`, and `cargo build --locked` on Ubuntu and Windows.

### Pinned Dependencies

- `Cargo.lock` is committed to version control.
- Builds use `cargo build --locked` for reproducible versions.

### Dependency List

Current direct dependencies (see [cli/Cargo.toml](cli/Cargo.toml)):

| Crate | Version | Purpose |
|-------|---------|---------|
| `anyhow` | 1.0 | Error handling |
| `clap` | 4.5 | CLI argument parsing |
| `command-group` | 5.0 | Process-group/job-object kill for spawned commands (see Process Lifecycle) |
| `crossterm` / `ratatui` | 0.28 | Terminal UI |
| `dirs` | 5.0 | Config/data paths |
| `ollama-rs` | 0.3 | Ollama client for `vn ai` |
| `reqwest` | 0.12 (rustls, no default features) | HTTP client |
| `rmcp` | 2 | Official MCP SDK - `vn mcp serve` and the TUI's embedded server (see MCP Host) |
| `hyper-util` / `schemars` | 0.1 / 1 | HTTP serving / JSON-schema generation for `rmcp` |
| `serde` / `serde_json` / `toml` | 1.0 / 1.0 / 0.8 | Config + session serialization |
| `sysinfo` | 0.31 | `vn sys info` |
| `tokio` | 1.40 | Async runtime |
| `chrono` | 0.4 | Timestamps |
| `futures-util` | 0.3 | Streaming |
| `tray-item` / `windows-sys` | 0.10 / 0.48 | Windows-only tray agent |

### Privilege Requirements

**`vn` does NOT require root or admin privileges for normal operation.**
The Windows launcher (`run_cli.bat`) runs unelevated; the tray offers a separate
"Open Admin TUI Terminal" (UAC prompt) only for tasks that need it.

| Command | Privilege | Notes |
|---------|-----------|-------|
| `vn sys info` | None | Read-only system information |
| `vn docker ...` / Open-menu apps | Docker daemon access | `docker` group (Linux) or Docker Desktop (Windows) |
| `vn git sync\|status` | None | Operates on your own repositories |
| `vn net scan` | None | RustScan TCP connect scan (see Network Access) |
| `vn run <script>` | Depends on script | Some scripts install packages (may prompt for sudo/UAC) |

---

## Script Execution Model

`vn run <name>` does **not** execute arbitrary paths or shell strings. Script
names map through a **static allow-list** in
[cli/crates/vn/src/commands/run.rs](cli/crates/vn/src/commands/run.rs) to fixed
relative paths under `scripts/` in the detected repo root (validated to contain
`.git/` + `scripts/`). `.bat` files run via `cmd /C`; `.sh` files run via
`wsl bash` on Windows. UNC paths are rejected for WSL translation. User input is
never interpolated into a command line; `vn docker up|down <name>` validates the
service name against Docker's allowed character set.

---

## Dockerized Web Apps

The TUI Open menu launches local web apps in Docker. Their shared posture:

- **Loopback-only exposure:** every app publishes its ports as
  `-p 127.0.0.1:<port>:<port>`, so nothing is reachable from the LAN. If you
  deliberately expose one, put authentication in front of it first.
- **Locally built apps run as a non-root user** (uid 10001 baked into the
  image; on Linux `vn` passes `--user $(id -u):$(id -g)` so files written to
  bind mounts stay owned by you), with `--cap-drop ALL`,
  `--security-opt no-new-privileges`, and `--pids-limit 512`.
- **Pulled vendor images (BentoPDF) are not cap-dropped**: some vendor
  entrypoints legitimately need privilege transitions (e.g. a root-to-app-user
  `setpriv` step requiring CAP_SETUID/SETGID) that break under
  `--cap-drop ALL`. They run as upstream intends, protected by the
  loopback-only binding.
- All of this is enforced by **one Rust code path**
  ([cli/crates/vn/src/commands/apps.rs](cli/crates/vn/src/commands/apps.rs), the
  `vn app open|stop` engine) instead of per-OS scripts, so the posture cannot
  drift between platforms.

Per-app threat model:

| App | Port | Mounts | Notes |
|-----|------|--------|-------|
| library | 8090 | `library/` read-write | Unauthenticated UI that can rename/move/delete PDFs - safe only because it is loopback-bound. State kept in `library/.portal/`. |
| doc-processor | 8085/8086 | host Desktop read-write | pandoc/tectonic markdown-to-PDF + pypdf join; output confined to the Desktop mount. |
| media-downloader | 8095 | host Desktop read-write | Fetches **untrusted URLs** (yt-dlp): http/https only, an initial fast-fail check rejects hosts resolving to loopback/private/link-local ranges, and **every connection yt-dlp itself makes is routed through an in-container egress-guard proxy** (`start_egress_proxy` in `docker/media-downloader/app.py`) that re-resolves and re-validates the target at actual connect time — this catches redirects to a different (private) host and DNS-rebinding between the initial check and yt-dlp's own lookup, not just the initial URL. Also: `--ignore-config --restrict-filenames --no-exec --max-filesize`, sanitized traversal-checked collision-safe output names. |
| BentoPDF / translator / docs | 8080/5000/3000 | none | Pulled published images (BentoPDF, translator) and a local build (docs). None has built-in authentication (translator can be gated with `LT_API_KEYS`); safe only while loopback-bound. Translation runs entirely on-container, no outbound requests per translation. |

Image supply chain: base images are pinned tags (`debian:12-slim`,
`python:3.12-slim`); Python deps are version-pinned except `yt-dlp`, which is
deliberately installed latest at build time (stale extractors break against
current sites). tectonic is a pinned release binary downloaded over HTTPS.

---

## MCP Host

vncli exposes its own host-control functions (currently: list/open/stop the Dockerized
apps) as an MCP server via the official [`rmcp`](https://crates.io/crates/rmcp) SDK - see
[AGENTS.md](AGENTS.md#mcp-host) for the architecture. Security-relevant properties:

- **Loopback-only by default.** The TUI's embedded server and `vn mcp serve --http` both
  bind `127.0.0.1` only, matching every other vncli-managed service in this document.
  `vn mcp serve` (stdio, no `--http`) never opens a network port at all - it's spawned as a
  subprocess by the MCP client (Claude Desktop/Code).
- **v1 exposes one toolset (Apps) only** - `list_apps`, `open_app`, `stop_app`. There is no
  arbitrary command execution, filesystem, or process-control tool exposed to MCP clients
  or the local model in this version.
- **`stop_app` requires human approval.** Any MCP client - external, or vncli's own
  Ollama chat calling the identical in-process tool - must have that call approved in the
  vncli TUI (`ApprovalGate`, `InputPurpose::ApproveMcp`) before it runs. `list_apps`/
  `open_app` are not gated, matching the TUI's own unconfirmed "vn app open" menu items.
- **Fail-closed when headless.** A standalone `vn mcp serve` (no TUI attached) auto-denies
  every `stop_app` request rather than hanging or silently allowing it - there is no
  console free to prompt on, since stdio is the protocol channel itself.
- **`open_app` defaults to not opening a browser** when called via MCP (`no_open: true`
  unless the caller explicitly passes `false`) - an LLM popping a browser window is a
  surprising side effect the human didn't ask for.

---

## Filesystem Access

| Path (Unix) | Path (Windows) | Permission | Purpose |
|-------------|----------------|-----------|---------|
| `~/.config/vn/config.toml` | `%APPDATA%\vn\config.toml` | `0o600` on Unix (Windows inherits profile ACLs) | CLI configuration |
| `~/.local/share/vn/sessions/` | `%LOCALAPPDATA%\vn\sessions\` | `0o700` on Unix | `vn ai` chat session history (plaintext JSON) |
| `<repo>/logs/` | same | user | TUI session logs (gitignored) |
| `<repo>/scripts/` | same | read/execute | Allow-listed task scripts |
| `<repo>/library/` | same | read-write (library only) | PDF library (gitignored, never enters images) |

**Session history and TUI logs are plaintext.** Treat them as sensitive; they
are gitignored - do not commit or share them.

---

## Network Access

| Feature | Default | Network Access |
|---------|---------|----------------|
| `vn ai` | On demand | Ollama at `http://127.0.0.1:11434` (localhost only; other hosts require explicit config) |
| `vn net scan` | On demand | **Actively scans** the local /24 subnet (common ports) or a given target with RustScan - only scan networks you own or have permission to test |
| `vn git sync` | On demand | Git remotes over HTTPS/SSH |
| `vn docker` / Open apps | On demand | Docker daemon; image pulls from Docker Hub/GHCR; media-downloader fetches user-supplied URLs (guarded, see above) |
| TUI (always) / `vn mcp serve --http` | On demand | MCP server on loopback `127.0.0.1:7332` (see MCP Host) - never reachable off-host |
| `vn mcp serve` (stdio) | On demand | None - no network port; runs as a subprocess of an MCP client over stdin/stdout |
| `vn sys` | - | None |

---

## Additional Resources

- [Rust Secure Code Working Group](https://github.com/rust-secure-code/wg)
- [RustSec Advisory Database](https://rustsec.org/)
- [OWASP: Secure Coding Practices](https://owasp.org/www-project-secure-coding-practices-quick-reference-guide/)
