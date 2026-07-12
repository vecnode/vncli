//! Shared plumbing for report-based MCP tools (every `apps::*_reported`
//! function's shape: do work, call a `&mut dyn FnMut(&str)` per progress
//! line). Used by both `apps_toolset.rs` and `docker_toolset.rs` so neither
//! hand-rolls the spawn_blocking/collect-lines/build-`CallToolResult` dance,
//! and so both stream progress the same way to in-process callers.

use crate::mcp::approval::ApprovalGate;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::ErrorData as McpError;
use std::sync::Arc;

/// Sink for progress lines as a report-based tool call produces them.
/// Supplied only by in-process callers - currently just the TUI's chat
/// integration, via `AppsToolset::call_by_name` - that want to see output as
/// it happens instead of only the final joined result once the whole call
/// (e.g. a multi-minute docker build) completes. External MCP clients
/// (stdio/HTTP `tools/call`) never get one: MCP's tool-call protocol is
/// request/response, so they only ever see the final `CallToolResult`.
pub(crate) type LiveReporter = Arc<dyn Fn(&str) + Send + Sync>;

/// Run a blocking, report-collecting fn (the shape every `apps::*_reported`
/// function has) on a blocking task: forwards each line to `live` (if any) as
/// it's produced, and joins the captured lines into the final
/// `CallToolResult` once done - success on `Ok`, the captured lines plus the
/// error on `Err`, matching what every hand-rolled version of this used to do
/// individually.
pub(crate) async fn run_reported(
    live: Option<LiveReporter>,
    f: impl FnOnce(&mut dyn FnMut(&str)) -> anyhow::Result<()> + Send + 'static,
) -> Result<CallToolResult, McpError> {
    let (outcome, lines) = tokio::task::spawn_blocking(move || {
        let mut lines = Vec::new();
        let mut report = |line: &str| {
            lines.push(line.to_string());
            if let Some(live) = &live {
                live(line);
            }
        };
        let outcome = f(&mut report);
        (outcome, lines)
    })
    .await
    .map_err(|err| McpError::internal_error(format!("task join error: {err}"), None))?;

    match outcome {
        Ok(()) => Ok(CallToolResult::success(vec![ContentBlock::text(
            lines.join("\n"),
        )])),
        Err(err) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
            "{err:#}\n{}",
            lines.join("\n")
        ))])),
    }
}

/// Request approval for a destructive tool call, returning the standard
/// "denied" `CallToolResult` if the user (or the fail-closed headless
/// default) doesn't approve - shared so every gated tool
/// (`stop_app`/`restart_app`/`docker_stop_all`/`docker_remove_containers`/
/// `docker_remove_images`) produces identical denial text instead of each
/// repeating it.
pub(crate) async fn require_approval(
    approval: &ApprovalGate,
    description: String,
) -> Option<CallToolResult> {
    if approval.request(description).await {
        None
    } else {
        Some(CallToolResult::error(vec![ContentBlock::text(
            "Denied: this action requires user approval in the vncli TUI.",
        )]))
    }
}

/// Format `rows` (each row's cells; `rows[0]` is the header row) as a
/// plain-text table with space-padded, fixed-width columns - not
/// tab-separated. A raw `\t` renders fine in a real terminal (which expands
/// tabs to stops) but misaligns badly inside the TUI's ratatui `Paragraph`:
/// it doesn't do tab-stop expansion, so columns drift, and combined with
/// word-wrap this can visually overlap with unrelated lines rather than just
/// looking mis-indented. Every column is sized to its own widest cell, so
/// this reads correctly both there and in a real terminal/log file.
pub(crate) fn format_table(rows: &[Vec<String>]) -> String {
    let Some(cols) = rows.first().map(Vec::len) else {
        return String::new();
    };
    let mut widths = vec![0usize; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, cell)| {
                    // No trailing padding on the last column - nothing to
                    // align after it, and it avoids invisible trailing
                    // whitespace on every row.
                    if i + 1 == cols {
                        cell.clone()
                    } else {
                        format!("{cell:width$}", width = widths[i])
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
