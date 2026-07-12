# vncli MCP functions

Human-readable tracker for every tool `vn`'s MCP host exposes. Update this file in the
same PR whenever a tool is added, removed, renamed, or its behavior changes materially.
See [AGENTS.md](../AGENTS.md#mcp-host) for how the toolsets are wired together in code.

Reachable via: `vn mcp serve` (stdio), `vn mcp serve --http` (loopback HTTP), the TUI's
embedded server (same HTTP transport, `127.0.0.1:7332`), and vncli's own in-TUI Ollama
chat (in-process, no MCP transport).

**Naming:** every docker-toolset tool is prefixed `docker_`, and every system-toolset tool is
prefixed `system_`, even the ones that could read fine without it (`docker_list_containers`,
not `list_containers`) — all tools share one flat `tools/list` namespace with the apps
toolset's `list_apps`/`open_app`/etc, so the prefix is what tells an LLM (or a human skimming
the list) whether a name means "docker-wide", "the whole host", or "one vncli app".

**Streaming:** in the TUI's own chat, tool calls that report progress (an `open_app` docker
build, `docker_remove_images`, etc.) stream each line into the CLI Output panel as it's
produced, not just as one lump result once the whole call finishes — see `LiveReporter` in
[mcp/report.rs](../cli/crates/vn/src/mcp/report.rs). External MCP clients (stdio/HTTP) don't
get this: MCP's `tools/call` is request/response, so they only ever see the final joined
result.

## Apps toolset (`cli/crates/vn/src/mcp/apps_toolset.rs`)

- `list_apps` — list the Dockerized vncli apps by name (docs, silverbullet, stirling-pdf, library-portal, media-downloader, doc-processor). Always allowed, no approval needed.
- `open_app` — build/start an app's container and wait for it to become ready; opens a browser tab unless `no_open` says otherwise (see default rules below). Always allowed, no approval needed.
- `stop_app` — stop a running app's container. **Destructive — requires interactive approval in the TUI**; auto-denied with no TUI attached (headless `vn mcp serve`).
- `restart_app` — stop_app + open_app in one call, for "it's stuck, restart it" instead of two tool calls. **Destructive — requires interactive approval in the TUI**; auto-denied with no TUI attached.

`open_app`/`restart_app`'s browser-opening default depends on the caller: headless/external MCP clients default to not opening a browser (`no_open` defaults to `true`); vncli's own in-TUI chat defaults to opening one (`no_open` defaults to `false`), using Chrome if installed, else the OS default browser. Pass `no_open` explicitly to override either way.

## Docker toolset (`cli/crates/vn/src/mcp/docker_toolset.rs`)

- `docker_list_containers` — list every container docker knows about on this host (running or stopped), with image, state, status, and published ports — not just vncli's own apps. Read-only, no approval needed.
- `docker_container_logs` — tail a container's stdout+stderr (defaults to the last 50 lines), or pass `pattern` (a regex) to search instead: scans the last 5000 lines and returns only matching lines (capped at 200 matches), so it can find things much further back than a plain tail reaches. Read-only, no approval needed.
- `docker_check` — confirm the daemon is running, list running containers (`docker ps`), and report total container/image counts. Read-only, no approval needed.
- `docker_stop_all` — stop *every* running container on the host, not just vncli's apps. **Destructive — requires interactive approval in the TUI**; auto-denied with no TUI attached.
- `docker_remove_containers` — stop and permanently remove every container on the host. **Destructive/irreversible — requires interactive approval in the TUI**; auto-denied with no TUI attached.
- `docker_remove_images` — permanently remove every docker image on the host. **Destructive/irreversible — requires interactive approval in the TUI**; auto-denied with no TUI attached.
- `docker_disk_usage` — how much disk space images/containers/volumes/build cache are using (wraps `docker system df`). Read-only, no approval needed.

`docker_stop_all`/`docker_remove_containers`/`docker_remove_images` act host-wide (anything docker has, not just vncli's own containers/images) and are hard or impossible to undo — gated the same way as `stop_app`, unlike the per-app tools which only ever touch vncli's own known containers.

## System toolset (`cli/crates/vn/src/mcp/system_toolset.rs`)

- `system_list_processes` — list processes running on the host's OS process table (not docker containers, not vncli apps): PID, name, executable path, parent PID, status, memory, and uptime for each. Optional `filter` param narrows by a case-insensitive substring of the name or executable path. Read-only, no approval needed.

Queried live via [`sysinfo`](https://crates.io/crates/sysinfo)'s native OS APIs (already a dependency — see `commands::sys`/`tray.rs`) rather than shelling out to `tasklist`/`ps`, so every call is a fresh snapshot with no subprocess-spawn overhead — each call reflects exactly what's running at that moment. An unfiltered call on a busy host is capped at 300 rows (with a note that it was truncated); pass `filter` to narrow it. There is no "watch mode" / continuous push — MCP's `tools/call` is request/response, so "real-time" here means each call is uncached and current, not a live stream; call it again to get the latest state.

## Adding a tool

New tools go on an existing `impl AppsToolset { #[tool_router(...)] ... }` block (or a new
one, for a new toolset — see `docker_toolset.rs` as the template). Add a line above, and a
matching `call_by_name`/`call_*_by_name` dispatch arm in the code — see AGENTS.md's
"To add a tool" note. If the tool reports progress as it runs (rather than completing
near-instantly), build it on `mcp/report.rs`'s `run_reported`/`require_approval` helpers so
it streams to the TUI's chat the same way the existing report-based tools do, and split it
into a thin `#[tool]` wrapper plus an `_impl(params, live: Option<LiveReporter>)` method
(see `open_app`/`open_app_impl` for the pattern).

## Ideas for future toolsets (not yet built)

Rough layering for where this could grow next, loosely ordered by how much they'd need
beyond what already exists in `commands::apps`:

- **App health/status** — an `app_status` tool combining `docker_ps_all` + the per-app `wait_port` from each `AppPlan` to answer "is doc-processor actually up" without opening a browser or re-running the whole `open_app` flow.
- **Library/file browsing** — read-only listing of `library/pdfs/` for the library-portal app, so a model could answer "what PDFs do I have" without a human opening the portal.

None of these are implemented yet — this section is a parking lot for scope, not a
commitment.
