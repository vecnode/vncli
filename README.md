# vncli

[![Language: Rust](https://img.shields.io/badge/language-Rust-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**An Open-Source Personal Management CLI for Researchers**

`vncli` is a cross-platform (Windows/Linux) command-line tool and TUI for researchers who want one local, offline-first place to manage their machine: sync personal git repositories, launch self-hosted research apps in Docker, chat with a local LLM over Ollama, and inspect system/network state — all from a single `vn` binary, no cloud account required.

## Install

Requires [Rust](https://rustup.rs/) and, for the Docker-managed apps, [Docker](https://docs.docker.com/engine/install/).

```
git clone https://github.com/vecnode/vncli.git
cd vncli
run_cli.bat      # Windows
./run_cli.sh     # Linux
```

Either launcher builds `vn` with cargo and drops you into the TUI. To produce a relocatable, prebuilt copy (no Rust toolchain required on the target machine), run `distribute_cli.bat` — it packages a ready-to-run folder onto your Desktop.

## Features

- Terminal UI tying everything below together.
- Pull/status across every git repo under a root folder (defaults to `~/dev`).
- Build/launch container applications
  - Docs
  - Library
  - Media Downloader
  - Doc Processor
  - Translator
- Direct Docker container/image management.
- Offline chat and model management against a local Ollama server.
- Host system information and maintenance.
- Local network port scanning via RustScan.
- MCP Server.

## Commands

- `vn tui` (default) — terminal UI tying everything below together.
- `vn git sync|status` — pull/status across every git repo under a root folder (defaults to `~/dev`).
- `vn app open|stop|list` — build/launch self-hosted research apps (docs, library, media-downloader, doc-processor, and pulled images like BentoPDF and LibreTranslate) as Docker containers, hardened and loopback-only by default.
- `vn docker ps|up|down|prune|check|stop-all|remove-containers|remove-images` — direct container/image management.
- `vn ai status|models|pull|chat` — offline chat and model management against a local [Ollama](https://ollama.com/) server.
- `vn sys info|update|clean` — host system information and maintenance.
- `vn net scan` — local network port scanning via RustScan.
- `vn mcp serve` — expose vncli's own tools (list/open/stop apps, docker/system introspection) as an MCP server over stdio or HTTP, for use from Claude Desktop/Code or vncli's own in-TUI chat.

## Downloading repositories

`scripts/win11/download_all_repos.bat` / `scripts/ubuntu22/download_all_repos.sh` clone every public repository for a given GitHub username; `download_all_orgs.*` does the same for one or more organizations. Both require the username/org(s) to be passed explicitly — there is no built-in default account.

## License

MIT - see [LICENSE](LICENSE). Author: vecnode.
