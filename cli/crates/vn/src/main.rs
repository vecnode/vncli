mod commands;
mod config;
mod mcp;
mod ollama;
mod tray;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "vn", version, about = "vncli cross-platform personal CLI")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true, hide = true)]
    repo_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Ai(AiArgs),
    App(AppArgs),
    Sys(SysArgs),
    Docker(DockerArgs),
    Git(GitArgs),
    Net(NetArgs),
    Run(RunArgs),
    Mcp(McpArgs),
    Tray,
    Tui,
}

#[derive(clap::Args, Debug)]
struct McpArgs {
    #[command(subcommand)]
    command: McpSubcommand,
}

#[derive(Subcommand, Debug)]
enum McpSubcommand {
    /// Serve vncli's MCP tools (currently: list/open/stop apps).
    ///
    /// Stdio (default) is for MCP clients that spawn their own subprocess
    /// (Claude Desktop/Code). With no TUI attached, destructive tool calls
    /// (stop_app) are auto-denied - there's no console free to approve them
    /// on, since stdio is the protocol channel itself.
    Serve {
        /// Serve over loopback HTTP (Streamable HTTP) instead of stdio.
        #[arg(long)]
        http: bool,
        /// Port to listen on when --http is set.
        #[arg(long, default_value_t = 7332)]
        port: u16,
    },
}

#[derive(clap::Args, Debug)]
struct AppArgs {
    #[command(subcommand)]
    command: AppCommand,
}

#[derive(Subcommand, Debug)]
enum AppCommand {
    /// Build/pull and start a Dockerized app, then open it in the browser.
    Open {
        /// App name (see `vn app list`).
        name: String,
        /// Do not open a browser after the app is ready.
        #[arg(long)]
        no_open: bool,
    },
    /// Stop an app's container (kept for fast reopen where applicable).
    Stop { name: String },
    /// List the available apps.
    List,
}

#[derive(clap::Args, Debug)]
struct AiArgs {
    #[command(subcommand)]
    command: AiCommand,

    /// Ollama host URL (defaults to the configured host).
    #[arg(long, global = true)]
    host: Option<String>,
}

#[derive(Subcommand, Debug)]
enum AiCommand {
    /// Check whether the Ollama server is reachable.
    Status,
    /// List locally installed models (one name per line).
    Models {
        /// Only list models that support tool/function calling (checked via
        /// `ollama show`'s reported capabilities). The TUI's chat always
        /// attaches tools, so a model without this capability can't chat.
        #[arg(long)]
        tools_only: bool,
    },
    /// Download a model by name (e.g. llama3.2).
    Pull { name: String },
    /// Send a chat message and print the reply (context kept per session).
    Chat {
        /// The message to send (pass as a single quoted argument).
        message: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        system: Option<String>,
    },
}

#[derive(clap::Args, Debug)]
struct RunArgs {
    name: String,
}

#[derive(clap::Args, Debug)]
struct NetArgs {
    #[command(subcommand)]
    command: Option<NetSubcommand>,
}

#[derive(Subcommand, Debug)]
enum NetSubcommand {
    /// Scan open ports with RustScan. Defaults to the local /24 subnet.
    Scan {
        /// Target to scan: IP, CIDR, or hostname. Defaults to the local /24 subnet.
        target: Option<String>,
    },
}

#[derive(clap::Args, Debug)]
struct SysArgs {
    #[command(subcommand)]
    command: Option<SysSubcommand>,
}

#[derive(Subcommand, Debug)]
enum SysSubcommand {
    Info,
    Update,
    Clean,
}

#[derive(clap::Args, Debug)]
struct DockerArgs {
    #[command(subcommand)]
    command: DockerSubcommand,
}

#[derive(Subcommand, Debug)]
enum DockerSubcommand {
    Ps,
    Up {
        service: Option<String>,
    },
    Down {
        service: Option<String>,
    },
    Prune,
    /// Check the Docker daemon and show container/image counts.
    Check,
    /// Stop every running container.
    StopAll,
    /// Stop and remove every container.
    RemoveContainers,
    /// Remove every image.
    RemoveImages,
}

#[derive(clap::Args, Debug)]
struct GitArgs {
    #[command(subcommand)]
    command: GitSubcommand,
}

#[derive(Subcommand, Debug)]
enum GitSubcommand {
    Sync {
        #[arg(long)]
        root: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = cli.repo_root.clone();
    let loaded = config::load_or_init(cli.config)?;

    match cli.command {
        Some(Command::Ai(args)) => commands::ai::run(args, &loaded).await?,
        Some(Command::App(args)) => match args.command {
            AppCommand::Open { name, no_open } => commands::apps::open(&name, &loaded, no_open)?,
            AppCommand::Stop { name } => commands::apps::stop(&name, &loaded)?,
            AppCommand::List => commands::apps::list()?,
        },
        Some(Command::Sys(args)) => commands::sys::run(args)?,
        Some(Command::Docker(args)) => commands::docker::run(args)?,
        Some(Command::Git(args)) => commands::git::run(args)?,
        Some(Command::Net(args)) => commands::net::run(args)?,
        Some(Command::Run(args)) => commands::run::run(args, &loaded)?,
        Some(Command::Mcp(args)) => commands::mcp::run(args, &loaded).await?,
        Some(Command::Tray) => tray::run(repo_root)?,
        Some(Command::Tui) | None => tui::app::run(repo_root, loaded)?,
    }

    Ok(())
}
