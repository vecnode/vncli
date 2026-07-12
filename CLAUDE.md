# CLAUDE.md

See [AGENTS.md](AGENTS.md) for the full guide to this repository (architecture, how the
`vn` CLI / TUI / Windows tray fit together, build & lint commands, how the TUI dispatches
to subcommands and scripts, and project conventions). It is the single source of truth
for both Claude and other agent tooling.

Quick reminders:

- Build after Rust changes: `cargo build --manifest-path cli/Cargo.toml -p vn`
- Lint: `cargo clippy --manifest-path cli/Cargo.toml -p vn`
- Run the TUI: `cargo run --manifest-path cli/Cargo.toml -p vn --`
- Open-port scan: `vn net scan` (uses RustScan; `cargo install rustscan` if missing)
- **`run_cli.bat` builds a *different* binary than the plain command above** (it passes
  `--target <host-triple>`, writing to `cli/target/<host-triple>/debug/vn.exe` instead of
  `cli/target/debug/vn.exe`). If you need to confirm a fix actually reaches what the user's
  launcher runs, rebuild with that same `--target` (see AGENTS.md's "Build, run, check") or
  just re-run `run_cli.bat` - don't assume a plain `cargo build` covers it.
