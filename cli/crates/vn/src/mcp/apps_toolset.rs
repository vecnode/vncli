//! The "Apps" MCP toolset: exposes `commands::apps` (open/stop/list the
//! Dockerized vncli apps) as MCP tools, reachable by external MCP clients
//! (Claude Desktop/Code) and by vncli's own Ollama chat (see
//! `tui/app.rs`'s chat worker). One `AppsToolset` instance backs both the
//! standalone `vn mcp serve` server and the TUI's embedded HTTP server -
//! there is exactly one implementation of these tools. The docker toolset
//! (`mcp/docker_toolset.rs`) is a second `#[tool_router]` impl block on this
//! same struct, merged in here (see `AppsToolset::new` and `call_by_name`).

use crate::commands::apps;
use crate::config::LoadedConfig;
use crate::mcp::approval::ApprovalGate;
use crate::mcp::report::{require_approval, run_reported, LiveReporter};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

/// Number of tools this toolset exposes (list_apps, open_app, stop_app,
/// restart_app), plus the docker and system toolsets merged into the same
/// struct - shown in the TUI's "MCP Server" panel.
pub const TOOL_COUNT: usize =
    4 + crate::mcp::docker_toolset::TOOL_COUNT + crate::mcp::system_toolset::TOOL_COUNT;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenAppParams {
    /// App name - see `list_apps` for the available names.
    pub name: String,
    /// Skip opening a browser once the app is ready. Omit to use the
    /// caller's default: an external/headless MCP client (`vn mcp serve`)
    /// defaults to true (an LLM popping your browser is a surprising side
    /// effect with no one watching); vncli's own in-TUI chat defaults to
    /// false, since the user is already present and asked for it. Pass this
    /// explicitly to override either way.
    pub no_open: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StopAppParams {
    /// App name - see `list_apps` for the available names.
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestartAppParams {
    /// App name - see `list_apps` for the available names.
    pub name: String,
    /// Skip opening a browser once the app is ready again. Same default
    /// rules as `open_app`'s `no_open` (see there).
    pub no_open: Option<bool>,
}

#[derive(Clone)]
pub struct AppsToolset {
    loaded: LoadedConfig,
    approval: ApprovalGate,
    /// Fallback for `open_app`'s `no_open` when the caller omits it - see
    /// `AppsToolset::new`.
    default_no_open: bool,
    /// Dispatch table built by `#[tool_router]`/read by `#[tool_handler]`'s
    /// generated `ServerHandler` methods below - the dead-code lint can't see
    /// that macro-generated read, hence the allow. Merges both toolsets
    /// implemented on this struct (apps lifecycle + docker introspection).
    #[allow(dead_code)]
    tool_router: ToolRouter<AppsToolset>,
}

impl AppsToolset {
    /// `default_no_open` is the fallback used when `open_app`'s `no_open`
    /// param is omitted: pass `true` for external/headless MCP callers (`vn
    /// mcp serve`, and the TUI's own embedded server for external clients),
    /// `false` for vncli's in-TUI Ollama chat (see module docs on
    /// `OpenAppParams::no_open`).
    pub fn new(loaded: LoadedConfig, approval: ApprovalGate, default_no_open: bool) -> Self {
        Self {
            loaded,
            approval,
            default_no_open,
            tool_router: Self::tool_router() + Self::docker_router() + Self::system_router(),
        }
    }

    /// Accessor for the docker toolset's gated tools (`docker_stop_all`,
    /// `docker_remove_containers`, `docker_remove_images`) - they live in a
    /// different module (`docker_toolset.rs`) and so can't reach the private
    /// `approval` field directly.
    pub(crate) fn approval(&self) -> &ApprovalGate {
        &self.approval
    }
}

#[tool_router]
impl AppsToolset {
    #[tool(
        description = "List the Dockerized vncli apps that can be opened or stopped (e.g. bentopdf, library, media-downloader, doc-processor, docs)."
    )]
    async fn list_apps(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            apps::APP_NAMES.join(", "),
        )]))
    }

    #[tool(
        description = "Build/start a Dockerized vncli app (creates and waits for the container to become ready)."
    )]
    async fn open_app(
        &self,
        Parameters(params): Parameters<OpenAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.open_app_impl(params, None).await
    }

    #[tool(
        description = "Stop a running vncli app's container. Requires the user to approve this in the vncli TUI; if there's no TUI attached, this is denied automatically."
    )]
    async fn stop_app(
        &self,
        Parameters(params): Parameters<StopAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.stop_app_impl(params, None).await
    }

    #[tool(
        description = "Restart a vncli app: stop its container (if running) and start/build it again in one call, instead of calling stop_app then open_app separately. Requires the user to approve this in the vncli TUI; if there's no TUI attached, this is denied automatically."
    )]
    async fn restart_app(
        &self,
        Parameters(params): Parameters<RestartAppParams>,
    ) -> Result<CallToolResult, McpError> {
        self.restart_app_impl(params, None).await
    }
}

impl AppsToolset {
    /// Shared implementation behind the `open_app` MCP tool and
    /// `call_by_name`'s streaming path. The `#[tool]` macro's generated wire
    /// dispatch can't carry an extra live-reporter parameter, so the thin
    /// `#[tool]` method above just calls this with `live: None`; `call_by_name`
    /// (the in-process Ollama chat path) passes `Some` so progress streams to
    /// the caller as it's produced instead of only at the end.
    async fn open_app_impl(
        &self,
        params: OpenAppParams,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        let no_open = params.no_open.unwrap_or(self.default_no_open);
        let loaded = self.loaded.clone();
        run_reported(live, move |report| {
            apps::open_reported(&params.name, &loaded, no_open, report)
        })
        .await
    }

    async fn stop_app_impl(
        &self,
        params: StopAppParams,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        let description = format!("stop_app(name=\"{}\")", params.name);
        if let Some(denied) = require_approval(&self.approval, description).await {
            return Ok(denied);
        }
        let loaded = self.loaded.clone();
        run_reported(live, move |report| {
            apps::stop_reported(&params.name, &loaded, report)
        })
        .await
    }

    async fn restart_app_impl(
        &self,
        params: RestartAppParams,
        live: Option<LiveReporter>,
    ) -> Result<CallToolResult, McpError> {
        let description = format!("restart_app(name=\"{}\")", params.name);
        if let Some(denied) = require_approval(&self.approval, description).await {
            return Ok(denied);
        }
        let no_open = params.no_open.unwrap_or(self.default_no_open);
        let loaded = self.loaded.clone();
        run_reported(live, move |report| {
            apps::stop_reported(&params.name, &loaded, report)
                .and_then(|()| apps::open_reported(&params.name, &loaded, no_open, report))
        })
        .await
    }
}

impl AppsToolset {
    /// The tool definitions (name/description/JSON schema) this toolset
    /// exposes - used by the Ollama chat integration (`tui/app.rs`) to build
    /// the same tool list an external MCP client would see via `tools/list`.
    pub fn list_tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }

    /// Call a tool by name with raw JSON arguments, streaming progress lines
    /// to `live` as a report-based tool call produces them (see
    /// `LiveReporter`) rather than only returning the joined result once the
    /// whole call finishes. Used by the Ollama chat integration, which
    /// discovers tools dynamically via `list_tools` and needs to invoke
    /// whichever one the model picked - unlike the MCP transports
    /// (stdio/HTTP), it has no `RequestContext` to route through
    /// `ToolRouter::call`, so this matches by name directly. New tools need
    /// an arm here too (or, for a whole new toolset, a `call_*_by_name`
    /// fallback like `call_docker_tool_by_name` below).
    pub async fn call_by_name(
        &self,
        name: &str,
        arguments: serde_json::Value,
        live: LiveReporter,
    ) -> Result<CallToolResult, McpError> {
        match name {
            "list_apps" => self.list_apps().await,
            "open_app" => {
                let params: OpenAppParams = serde_json::from_value(arguments).map_err(|err| {
                    McpError::invalid_params(format!("invalid open_app arguments: {err}"), None)
                })?;
                self.open_app_impl(params, Some(live)).await
            }
            "stop_app" => {
                let params: StopAppParams = serde_json::from_value(arguments).map_err(|err| {
                    McpError::invalid_params(format!("invalid stop_app arguments: {err}"), None)
                })?;
                self.stop_app_impl(params, Some(live)).await
            }
            "restart_app" => {
                let params: RestartAppParams =
                    serde_json::from_value(arguments).map_err(|err| {
                        McpError::invalid_params(
                            format!("invalid restart_app arguments: {err}"),
                            None,
                        )
                    })?;
                self.restart_app_impl(params, Some(live)).await
            }
            other => {
                if let Some(result) = self
                    .call_docker_tool_by_name(other, arguments.clone(), live)
                    .await
                {
                    return result;
                }
                match self.call_system_tool_by_name(other, arguments).await {
                    Some(result) => result,
                    None => Err(McpError::invalid_params(
                        format!("unknown tool: {other}"),
                        None,
                    )),
                }
            }
        }
    }
}

// `router = self.tool_router.clone()`: without this, `#[tool_handler]` defaults
// to calling `Self::tool_router()` fresh, which is only the apps-lifecycle
// router - it would silently drop the docker toolset merged into the
// instance field in `AppsToolset::new`.
#[tool_handler(router = self.tool_router.clone())]
impl ServerHandler for AppsToolset {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vncli", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "vncli host controller: list/open/stop/restart the Dockerized vncli apps, \
                 inspect/maintain docker on the host (docker_list_containers, \
                 docker_container_logs, docker_check, docker_disk_usage, docker_stop_all, \
                 docker_remove_containers, docker_remove_images), and list OS processes on \
                 the host (system_list_processes). stop_app, restart_app, and the destructive \
                 docker_* tools require interactive approval in the vncli TUI.",
            )
    }
}
