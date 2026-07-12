//! The "Docker" MCP toolset: introspection into and maintenance of whatever
//! docker actually has on the host, as opposed to `apps_toolset` (which only
//! ever deals with vncli's own [`APP_NAMES`](crate::commands::apps::APP_NAMES)).
//! Lets an MCP client (or vncli's own Ollama chat) answer "what containers
//! exist right now", "why did that one fail", "which lines mention 'error'",
//! and "how much space is this using" without shelling out itself, plus
//! wrappers for `vn docker`'s existing maintenance commands. Every tool name
//! is prefixed `docker_` -
//! deliberately, even though the module name already says "docker": these
//! tools sit in one flat namespace with the apps toolset's `list_apps`/
//! `open_app`/etc, so the prefix is what tells an LLM (or a human skimming
//! `tools/list`) that a name means "docker-wide", not "one vncli app".
//!
//! `docker_list_containers`, `docker_container_logs`, `docker_check` and
//! `docker_disk_usage` are read-only. `docker_stop_all`,
//! `docker_remove_containers` and `docker_remove_images` act host-wide (not
//! just on vncli's own containers/images) and are hard or impossible to
//! undo, so - like `stop_app` - they go through
//! [`ApprovalGate`](crate::mcp::approval::ApprovalGate) via
//! `AppsToolset::approval()`.
//!
//! This is a second `#[tool_router]` impl block on [`AppsToolset`] (rmcp
//! merges same-typed routers with `+`, see `AppsToolset::new`), not a
//! separate handler/server - `rmcp`'s `ServerHandler` is one struct per
//! transport, so a new toolset composes into the existing struct rather than
//! standing alongside it.

use crate::commands::apps;
use crate::mcp::report::{format_table, require_approval, run_reported, LiveReporter};
use crate::mcp::AppsToolset;
use anyhow::Context;
use regex::Regex;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router, ErrorData as McpError};
use schemars::JsonSchema;
use serde::Deserialize;

/// Number of tools this toolset exposes (docker_list_containers,
/// docker_container_logs, docker_check, docker_stop_all,
/// docker_remove_containers, docker_remove_images, docker_disk_usage) -
/// added to `apps_toolset::TOOL_COUNT` for the TUI's "MCP Server" panel.
pub const TOOL_COUNT: usize = 7;

fn default_log_lines() -> u32 {
    50
}

/// How far back `docker_container_logs` looks when `pattern` is set - a
/// search is only useful if it can reach further back than the plain
/// (unfiltered) default tail of `default_log_lines()`, so this is much
/// larger than that.
const SEARCH_SCAN_LINES: u32 = 5000;

/// Cap on matching lines returned for a `pattern` search, so a pattern that
/// matches nearly everything (e.g. an empty-ish regex) doesn't dump
/// thousands of lines into the model's context.
const MAX_MATCHES: usize = 200;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContainerLogsParams {
    /// Container name or ID - see `docker_list_containers` for what exists.
    pub name: String,
    /// Number of trailing log lines to return. Ignored (see `pattern`
    /// instead) when `pattern` is set. Defaults to 50.
    #[serde(default = "default_log_lines")]
    pub lines: u32,
    /// Regex to search for instead of returning a plain tail - only matching
    /// lines are returned (case-sensitive; prefix with `(?i)` for
    /// case-insensitive, e.g. `(?i)error`). Searches the last
    /// `SEARCH_SCAN_LINES` (5000) lines rather than just `lines`, since the
    /// point of a search is to reach further back than the default tail.
    /// Capped at `MAX_MATCHES` (200) matching lines.
    pub pattern: Option<String>,
}

#[tool_router(router = docker_router, vis = "pub(crate)")]
impl AppsToolset {
    #[tool(
        description = "List every docker container on this host (running or stopped), with image, state, status and published ports. Unlike list_apps, this reflects docker's actual state rather than vncli's known app names."
    )]
    async fn docker_list_containers(&self) -> Result<CallToolResult, McpError> {
        let containers = tokio::task::spawn_blocking(apps::docker_ps_all)
            .await
            .map_err(|err| McpError::internal_error(format!("task join error: {err}"), None))?
            .map_err(|err| McpError::internal_error(format!("{err:#}"), None))?;

        if containers.is_empty() {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "No containers found (docker ps -a returned nothing).",
            )]));
        }

        let mut rows = vec![vec![
            "ID".to_string(),
            "NAME".to_string(),
            "STATE".to_string(),
            "STATUS".to_string(),
            "IMAGE".to_string(),
            "PORTS".to_string(),
        ]];
        for c in containers {
            rows.push(vec![c.id, c.name, c.state, c.status, c.image, c.ports]);
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            format_table(&rows),
        )]))
    }

    #[tool(
        description = "Tail a container's logs (stdout+stderr), or search them with a regex via `pattern` to pull out matching lines from much further back than a plain tail would reach (e.g. pattern=\"(?i)error\" to find every error line). Use docker_list_containers first to find the container name."
    )]
    async fn docker_container_logs(
        &self,
        Parameters(params): Parameters<ContainerLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = tokio::task::spawn_blocking(move || search_or_tail_logs(&params))
            .await
            .map_err(|err| McpError::internal_error(format!("task join error: {err}"), None))?
            .map_err(|err| McpError::internal_error(format!("{err:#}"), None))?;

        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "Check docker's status: confirms the daemon is running, lists running containers (`docker ps`), and reports total container/image counts."
    )]
    async fn docker_check(&self) -> Result<CallToolResult, McpError> {
        self.docker_check_impl(None).await
    }

    #[tool(
        description = "Stop every running container on this host, not just vncli's own apps. Hard to undo cleanly if you don't know what else was running, so requires the user to approve this in the vncli TUI; if there's no TUI attached, this is denied automatically."
    )]
    async fn docker_stop_all(&self) -> Result<CallToolResult, McpError> {
        self.docker_stop_all_impl(None).await
    }

    #[tool(
        description = "Stop and permanently remove every container on this host, not just vncli's own apps. Irreversible (containers are deleted, not just stopped), so requires the user to approve this in the vncli TUI; if there's no TUI attached, this is denied automatically."
    )]
    async fn docker_remove_containers(&self) -> Result<CallToolResult, McpError> {
        self.docker_remove_containers_impl(None).await
    }

    #[tool(
        description = "Permanently remove every docker image on this host, not just vncli's own. Irreversible (rebuilt/re-pulled from scratch next time), so requires the user to approve this in the vncli TUI; if there's no TUI attached, this is denied automatically."
    )]
    async fn docker_remove_images(&self) -> Result<CallToolResult, McpError> {
        self.docker_remove_images_impl(None).await
    }

    #[tool(
        description = "Show how much disk space docker's images, containers, local volumes, and build cache are using (wraps `docker system df`)."
    )]
    async fn docker_disk_usage(&self) -> Result<CallToolResult, McpError> {
        let text = tokio::task::spawn_blocking(apps::docker_disk_usage)
            .await
            .map_err(|err| McpError::internal_error(format!("task join error: {err}"), None))?
            .map_err(|err| McpError::internal_error(format!("{err:#}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }
}

impl AppsToolset {
    /// Shared implementation behind the `docker_check` MCP tool and
    /// `call_by_name`'s streaming path - see `apps_toolset.rs`'s
    /// `open_app_impl` for why this split exists (the `#[tool]` macro can't
    /// carry an extra live-reporter parameter).
    async fn docker_check_impl(
        &self,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        run_reported(live, apps::docker_check_reported).await
    }

    async fn docker_stop_all_impl(
        &self,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(denied) =
            require_approval(self.approval(), "docker_stop_all()".to_string()).await
        {
            return Ok(denied);
        }
        run_reported(live, apps::docker_stop_all_reported).await
    }

    async fn docker_remove_containers_impl(
        &self,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(denied) =
            require_approval(self.approval(), "docker_remove_containers()".to_string()).await
        {
            return Ok(denied);
        }
        run_reported(live, apps::docker_remove_containers_reported).await
    }

    async fn docker_remove_images_impl(
        &self,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(denied) =
            require_approval(self.approval(), "docker_remove_images()".to_string()).await
        {
            return Ok(denied);
        }
        run_reported(live, apps::docker_remove_images_reported).await
    }
}

impl AppsToolset {
    /// Dispatch for the docker toolset's tools - called from
    /// `AppsToolset::call_by_name` alongside the apps toolset's own arms, so
    /// the Ollama chat integration can reach these tools too, streaming
    /// progress to `live` for the report-based ones.
    pub(super) async fn call_docker_tool_by_name(
        &self,
        name: &str,
        arguments: serde_json::Value,
        live: LiveReporter,
    ) -> Option<Result<CallToolResult, McpError>> {
        match name {
            "docker_list_containers" => Some(self.docker_list_containers().await),
            "docker_container_logs" => {
                let params: ContainerLogsParams = match serde_json::from_value(arguments) {
                    Ok(params) => params,
                    Err(err) => {
                        return Some(Err(McpError::invalid_params(
                            format!("invalid docker_container_logs arguments: {err}"),
                            None,
                        )))
                    }
                };
                Some(self.docker_container_logs(Parameters(params)).await)
            }
            "docker_check" => Some(self.docker_check_impl(Some(live)).await),
            "docker_stop_all" => Some(self.docker_stop_all_impl(Some(live)).await),
            "docker_remove_containers" => {
                Some(self.docker_remove_containers_impl(Some(live)).await)
            }
            "docker_remove_images" => Some(self.docker_remove_images_impl(Some(live)).await),
            "docker_disk_usage" => Some(self.docker_disk_usage().await),
            _ => None,
        }
    }
}

/// Runs on a blocking task (see `docker_container_logs`): without a
/// `pattern`, this is exactly the old plain-tail behavior
/// (`docker_logs_tail(name, lines)`). With one, it fetches a much larger
/// scan window (`SEARCH_SCAN_LINES`) and returns only the lines matching the
/// regex, capped at `MAX_MATCHES`.
fn search_or_tail_logs(params: &ContainerLogsParams) -> anyhow::Result<String> {
    let Some(pattern) = &params.pattern else {
        return apps::docker_logs_tail(&params.name, params.lines);
    };

    let re = Regex::new(pattern).with_context(|| format!("invalid `pattern` regex: {pattern}"))?;
    let raw = apps::docker_logs_tail(&params.name, SEARCH_SCAN_LINES)?;

    let mut matches: Vec<&str> = raw.lines().filter(|line| re.is_match(line)).collect();
    let total = matches.len();
    if matches.is_empty() {
        return Ok(format!(
            "No lines matching `{pattern}` in the last {SEARCH_SCAN_LINES} lines of {}'s logs.",
            params.name
        ));
    }
    matches.truncate(MAX_MATCHES);

    let mut text = matches.join("\n");
    if total > MAX_MATCHES {
        text.push_str(&format!(
            "\n... truncated to {MAX_MATCHES} of {total} matching lines; narrow `pattern` for fewer matches."
        ));
    }
    Ok(text)
}
