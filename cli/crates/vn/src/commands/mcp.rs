use crate::config::LoadedConfig;
use crate::mcp::approval::ApprovalGate;
use crate::mcp::AppsToolset;
use crate::{McpArgs, McpSubcommand};
use anyhow::{Context, Result};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::ServiceExt;

pub async fn run(args: McpArgs, loaded: &LoadedConfig) -> Result<()> {
    match args.command {
        McpSubcommand::Serve { http, port } => {
            // No TUI is attached to a standalone `vn mcp serve` process, so
            // destructive tool calls (stop_app) are auto-denied - fail-closed.
            if http {
                serve_http(loaded.clone(), port, ApprovalGate::headless(), false).await
            } else {
                serve_stdio(loaded.clone(), ApprovalGate::headless()).await
            }
        }
    }
}

/// Stdio transport: for MCP clients that spawn `vn mcp serve` themselves
/// (Claude Desktop/Code's usual local-server integration model). Takes an
/// `ApprovalGate` so both the standalone CLI subcommand (headless, fail-closed)
/// and (in principle) a TUI-attached caller share one implementation.
pub async fn serve_stdio(loaded: LoadedConfig, approval: ApprovalGate) -> Result<()> {
    // External/headless caller: default to not popping a browser (see
    // `OpenAppParams::no_open`).
    let toolset = AppsToolset::new(loaded, approval, true);
    let service = toolset
        .serve(stdio())
        .await
        .context("failed to start MCP stdio server")?;
    service.waiting().await.context("MCP stdio server error")?;
    Ok(())
}

/// Loopback-only Streamable HTTP transport. Used both by the headless
/// `vn mcp serve --http` subcommand (fail-closed `ApprovalGate::headless()`)
/// and the TUI's embedded server (`tui/app.rs`, sharing its own `ApprovalGate`
/// so destructive tool calls surface as an approval prompt in the TUI).
pub async fn serve_http(
    loaded: LoadedConfig,
    port: u16,
    approval: ApprovalGate,
    quiet: bool,
) -> Result<()> {
    // Both callers of `serve_http` (standalone `vn mcp serve --http` and the
    // TUI's embedded server for *external* MCP clients) are headless from
    // this toolset's point of view - no one is necessarily watching, so
    // default to not popping a browser (see `OpenAppParams::no_open`).
    let service = TowerToHyperService::new(StreamableHttpService::new(
        move || Ok(AppsToolset::new(loaded.clone(), approval.clone(), true)),
        LocalSessionManager::default().into(),
        Default::default(),
    ));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("failed to bind MCP HTTP server on 127.0.0.1:{port}"))?;
    if !quiet {
        println!("[OK] MCP server listening on http://127.0.0.1:{port}");
    }

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .context("failed to accept connection")?;
        let io = TokioIo::new(stream);
        let service = service.clone();
        tokio::spawn(async move {
            let _ = Builder::new(TokioExecutor::default())
                .serve_connection(io, service)
                .await;
        });
    }
}
