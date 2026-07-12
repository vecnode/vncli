//! vncli's MCP (Model Context Protocol) host: exposes vn's own host-control
//! functions as MCP tools, reachable by external MCP clients (Claude
//! Desktop/Code) via `vn mcp serve`, and by vncli's own Ollama chat
//! in-process (see `tui/app.rs`). Three toolsets so far - apps lifecycle
//! (`apps_toolset`), docker introspection/maintenance (`docker_toolset`), and
//! OS process introspection (`system_toolset`) - all implemented as
//! `#[tool_router]` blocks on the same `AppsToolset` struct (rmcp merges
//! same-typed routers with `+`; see `AppsToolset::new`). To add another: same
//! pattern, a new `#[tool_router(router = ...)]` impl block, merged in
//! `AppsToolset::new` and dispatched from `call_by_name`. `report` holds
//! plumbing shared by the report-based tools (see its docs for
//! `run_reported`/`require_approval`/`LiveReporter`).

pub mod approval;
pub mod apps_toolset;
pub mod docker_toolset;
mod report;
pub mod system_toolset;

pub use apps_toolset::AppsToolset;
pub(crate) use report::LiveReporter;
