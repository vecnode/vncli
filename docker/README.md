# docker/

Independent build folders for the self-hosted web apps vncli's TUI **Open** menu
(and `vn app open|stop|list`) launches in Docker. Each app is one folder here plus one
`plan_for` arm in [cli/crates/vn/src/commands/apps.rs](../cli/crates/vn/src/commands/apps.rs)
— that's the single, cross-platform engine that builds/pulls the image, starts the
container, waits for it to become ready, and opens the browser. There are no per-OS
launch scripts for these apps.

| Folder | App | Port(s) | What it does |
|--------|-----|---------|---------------|
| [media-processor/](media-processor/) | doc-processor | 8085 (UI) / 8086 (API) | Markdown → PDF via pandoc + [tectonic](https://tectonic-typesetting.github.io/) |
| [library-portal/](library-portal/) | library | 8090 | Viewer/manager for the repo's `library/` folder — edit, tag, list/grid/tree views, drag-and-drop |
| [media-downloader/](media-downloader/) | media-downloader | 8095 | yt-dlp + ffmpeg web UI — paste a URL, save MP3/WAV/MP4 to the host Desktop |

Each folder has its own `README.md` with manual `docker build`/`run` commands, but the
normal way to run any of these is through `vn`:

```bash
vn app open <name>    # e.g. vn app open library
vn app stop <name>
vn app list
```

`docs/` (mdBook) and SilverBullet/BentoPDF (pulled vendor images) are also part of
the same `vn app` registry but don't have a folder here — the first has its own
[docs/Dockerfile](../docs/Dockerfile), and the latter two are pulled, not built.

## Security posture

All of these are **loopback-only** (`127.0.0.1:<port>`, never reachable off-host), and
the locally-built ones (everything in this table) run non-root with `--cap-drop ALL`,
`--security-opt no-new-privileges`, and a pids limit. media-downloader additionally
routes `yt-dlp` through an in-container egress-guard proxy, since it fetches arbitrary
untrusted URLs. Full details, per-app threat model, and the reasoning behind each choice
are in [SECURITY.md](../SECURITY.md#dockerized-web-apps).

## Controlling apps via MCP

These same apps (list/open/stop) are also exposed as MCP tools — vncli is an MCP host,
so an external MCP client (Claude Desktop/Code) or vncli's own Ollama chat can call
`list_apps`/`open_app`/`stop_app` directly. See
[AGENTS.md](../AGENTS.md#mcp-host) for how that's wired up and how to add a tool for a
new app.

## Adding a new app

1. Add the Dockerfile (and any UI assets) in a new folder here.
2. Add one `plan_for` arm in
   [commands/apps.rs](../cli/crates/vn/src/commands/apps.rs) — image, container name,
   build context, lifecycle, ports, mounts, readiness port. The engine enforces the
   hardening flags automatically; don't bypass it.
3. Add a `CommandItem` to both Open submenus in
   [tui/app.rs](../cli/crates/vn/src/tui/app.rs).
4. Write this folder's own `README.md` following the pattern in the existing ones.
