//! The "System" MCP toolset: read-only introspection into OS-level processes
//! on the host - not docker containers (see `docker_toolset`), not vncli's
//! own apps (see `apps_toolset`), but every process the OS knows about.
//! Queried live via [`sysinfo`](https://crates.io/crates/sysinfo)'s native OS
//! APIs (already a dependency - see `commands::sys`/`tray.rs`) rather than
//! shelling out to `tasklist`/`ps`, so a call reflects exactly what's running
//! at that moment with no subprocess-spawn overhead in between.
//!
//! Every tool here is prefixed `system_`, for the same reason
//! `docker_toolset`'s tools are prefixed `docker_`: all toolsets share one
//! flat `tools/list` namespace with `list_apps`/`open_app`/etc, so the prefix
//! tells an LLM (or a human) a name means "the whole host", not "one vncli
//! app" or "one docker container". `system_list_processes` is read-only, so
//! it doesn't go through [`ApprovalGate`](crate::mcp::approval::ApprovalGate).

use crate::mcp::report::format_table;
use crate::mcp::AppsToolset;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;
use sysinfo::{ProcessesToUpdate, System};

/// Number of tools this toolset exposes (system_list_processes) - added to
/// `apps_toolset::TOOL_COUNT` for the TUI's "MCP Server" panel.
pub const TOOL_COUNT: usize = 1;

/// Hard cap on rows returned in one call - an unfiltered scan of a busy
/// Windows host can be several hundred processes; past this it's more useful
/// to nudge the caller toward `filter` than to dump everything into the
/// model's context.
const MAX_ROWS: usize = 300;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProcessesParams {
    /// Only include processes whose name or executable path contains this
    /// text (case-insensitive) - e.g. "docker", "ollama", "chrome". Omit to
    /// list every process on the host (capped, see the tool description).
    pub filter: Option<String>,
}

#[tool_router(router = system_router, vis = "pub(crate)")]
impl AppsToolset {
    #[tool(
        description = "List processes currently running on this host - the OS process table, not docker containers or vncli's own apps. Reports PID, name, executable path, parent PID, status, memory, and uptime for each. Queried live via native OS APIs on every call (no caching), so it always reflects what's running right now. Optionally filter by a case-insensitive substring of the name or executable path. Unfiltered results are capped at 300 rows - pass filter to narrow a busy host."
    )]
    async fn system_list_processes(
        &self,
        Parameters(params): Parameters<ListProcessesParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = tokio::task::spawn_blocking(move || list_processes_text(params.filter))
            .await
            .map_err(|err| McpError::internal_error(format!("task join error: {err}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }
}

impl AppsToolset {
    /// Dispatch for the system toolset's tools - called from
    /// `AppsToolset::call_by_name` alongside the apps/docker toolsets' own
    /// arms, so the Ollama chat integration can reach these tools too.
    pub(super) async fn call_system_tool_by_name(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Option<Result<CallToolResult, McpError>> {
        match name {
            "system_list_processes" => {
                let params: ListProcessesParams = match serde_json::from_value(arguments) {
                    Ok(params) => params,
                    Err(err) => {
                        return Some(Err(McpError::invalid_params(
                            format!("invalid system_list_processes arguments: {err}"),
                            None,
                        )))
                    }
                };
                Some(self.system_list_processes(Parameters(params)).await)
            }
            _ => None,
        }
    }
}

/// Runs on a blocking task (see `system_list_processes`): `sysinfo`'s process
/// refresh does blocking syscalls (`/proc` reads on Linux, native APIs on
/// Windows), so it doesn't belong on the async executor.
fn list_processes_text(filter: Option<String>) -> String {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All);

    let filter = filter.map(|f| f.to_lowercase());
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut rows: Vec<(u32, Vec<String>)> = Vec::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_string();
        let exe = process
            .exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        if let Some(filter) = &filter {
            if !name.to_lowercase().contains(filter.as_str())
                && !exe.to_lowercase().contains(filter.as_str())
            {
                continue;
            }
        }

        let parent = process
            .parent()
            .map(|p| p.as_u32().to_string())
            .unwrap_or_default();
        let uptime_secs = now_secs.saturating_sub(process.start_time());
        let memory_mb = process.memory() / 1024 / 1024;

        rows.push((
            pid.as_u32(),
            vec![
                pid.as_u32().to_string(),
                name,
                parent,
                format!("{:?}", process.status()),
                format!("{memory_mb}MB"),
                format!("{uptime_secs}s"),
                exe,
            ],
        ));
    }

    if rows.is_empty() {
        return "No matching processes found.".to_string();
    }
    rows.sort_by_key(|(pid, _)| *pid);

    let total = rows.len();
    rows.truncate(MAX_ROWS);

    let mut table_rows = vec![vec![
        "PID".to_string(),
        "NAME".to_string(),
        "PPID".to_string(),
        "STATUS".to_string(),
        "MEMORY".to_string(),
        "UPTIME".to_string(),
        "EXE".to_string(),
    ]];
    table_rows.extend(rows.into_iter().map(|(_, row)| row));

    let mut text = format_table(&table_rows);
    if total > MAX_ROWS {
        text.push_str(&format!(
            "\n... truncated to {MAX_ROWS} of {total} matching processes; pass `filter` to narrow the results."
        ));
    }
    text
}
