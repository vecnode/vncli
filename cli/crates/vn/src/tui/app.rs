use crate::commands::mcp as mcp_command;
use crate::config::{expand_tilde, LoadedConfig};
use crate::mcp::approval::{ApprovalGate, PendingApproval};
use crate::mcp::AppsToolset;
use crate::ollama::session::{session_path, SessionFile};
use anyhow::Result;
use chrono::Local;
use command_group::{CommandGroup, GroupChild};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ollama_rs::generation::chat::{request::ChatMessageRequest, ChatMessage};
use ollama_rs::generation::tools::{ToolFunctionInfo, ToolInfo, ToolType};
use ollama_rs::models::ModelOptions;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table, Wrap};
use ratatui::Terminal;
use std::collections::VecDeque;
use std::env;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::io::{self, Stdout};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

// ---- Theme: one cyan accent (the Docker header / button blue) on the
// terminal's (black) background, plus two grays. Two "blues" total: this cyan
// used as a fill (selection / focus) and as text (headers, commands). ----
const ACCENT: Color = Color::Cyan; // the single blue accent
const DIM: Color = Color::DarkGray; // de-emphasized borders/text
const MUTED: Color = Color::Gray; // secondary text / inactive titles
                                  // CLI Output only adds two source-tag colors on top of the above (see
                                  // `tagged_line_color`): Magenta for `[MCP]` lines, Blue for `[DOCKER]` lines.
                                  // Existing severity colors (Error=LightRed, Stderr=Yellow, Command=ACCENT)
                                  // are untouched by this - it only recolors otherwise-plain Stdout/Info text.

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Root,
    RunUbuntu22,
    RunUbuntu22Network,
    RunUbuntu22Dependencies,
    RunUbuntu22Github,
    RunUbuntu22Open,
    RunUbuntu22Ai,
    RunUbuntu22Dotfiles,
    RunWin11,
    RunWin11Network,
    RunWin11Dependencies,
    RunWin11Github,
    RunWin11Open,
    RunWin11Ai,
    RunWin11Dotfiles,
    SelectModel,
}

/// What a typed line in the input box should do when submitted.
#[derive(Clone, PartialEq)]
enum InputPurpose {
    None,
    DownloadModel,
    Chat,
    /// Armed by `Action::ExecuteConfirm`: only runs `args` (labelled `label`)
    /// if the typed line is "yes"; anything else cancels.
    ConfirmDestructive(Vec<&'static str>, &'static str),
    /// Armed by a pending MCP tool-call approval (`AppState.active_mcp_respond`
    /// holds the actual oneshot reply channel; this variant just carries the
    /// description text for display, since a `oneshot::Sender` can't derive
    /// `Clone`/`PartialEq`).
    ApproveMcp(String),
}

#[derive(Clone)]
enum Action {
    Execute(Vec<&'static str>),
    /// Like `Execute`, but requires typing "yes" in the input box first.
    /// Used for destructive commands (stop/remove all containers or images).
    ExecuteConfirm(Vec<&'static str>),
    OpenMenu(MenuKind),
    BackToRoot,
    /// Query Ollama for installed models and open the model-selection menu.
    OpenModelMenu,
    /// Focus the input box and route the next submitted line to this purpose.
    ArmInput(InputPurpose),
}

#[derive(Clone)]
struct CommandItem {
    label: &'static str,
    action: Action,
}

enum ProcEvent {
    Stdout(String),
    Stderr(String),
    /// A reply from the persistent chat worker thread (see `spawn_chat_worker`).
    ChatReply(std::result::Result<String, String>),
    /// An intermediate MCP tool-call/result line from the chat's tool-calling
    /// loop, shown distinctly from the final chat reply.
    McpActivity(String),
}

/// Sent to the persistent chat worker thread.
enum ChatRequest {
    /// One chat turn.
    Turn {
        model: Option<String>,
        message: String,
    },
    /// Drop the in-memory/on-disk "tui" session's message history. Sent when
    /// the active model changes (see `activate_selected`) - without this, a
    /// newly-selected model inherits whatever the *previous* model said in
    /// this same conversation, including any fabricated ("hallucinated")
    /// replies, and tends to treat that prior turn as established fact
    /// rather than independently re-checking it.
    ResetSession,
}

enum LogEntry {
    Command(String),
    Info(String),
    Error(String),
    Stdout(String),
    Stderr(String),
    /// One line from a `ProcEvent::McpActivity` event - a `[MCP] Calling
    /// .../[MCP] Result: ...` tag line, or one of the (untagged)
    /// continuation lines `split_to_entries` breaks a multi-line result
    /// (e.g. a table) into. Its own variant, not `Info`, so every line an
    /// MCP tool call produced renders in the same Magenta - previously only
    /// the one line that happened to literally start with `[MCP]` got
    /// colored, leaving every other line from that same call grey with no
    /// visual link back to it.
    Mcp(String),
}

enum Focus {
    Dashboard,
    /// The "Running" panel listing background processes; lets you select and
    /// kill one individually.
    Running,
    Input,
}

struct DockerPanelData {
    available: bool,
    /// One row per running container: (port, image, container name).
    rows: Vec<(String, String, String)>,
}

struct RunningProcess {
    label: String,
    /// Spawned via `group_spawn()` so it owns its own process group (Unix) /
    /// job object (Windows): killing it also kills any children it spawned
    /// (e.g. `docker build`, `yt-dlp`), instead of leaving them orphaned.
    child: GroupChild,
    started_at: Instant,
}

/// Cap on `AppState::logs` (the CLI Output panel's in-memory scrollback) -
/// `trim_logs()` drains older entries once this is exceeded. High enough
/// that a normal session's docker/MCP/chat output doesn't visibly vanish out
/// from under you mid-use (each entry is a short `String`, so even a few
/// thousand cost negligible memory) - the full, untrimmed history is always
/// still in `logs/vn-tui.log` regardless of this cap.
const MAX_LOG_ENTRIES: usize = 5000;

struct AppState {
    menu: MenuKind,
    commands: Vec<CommandItem>,
    selected: usize,
    repo_root: Option<std::path::PathBuf>,
    logs: Vec<LogEntry>,
    running: Vec<RunningProcess>,
    tx: Sender<ProcEvent>,
    rx: Receiver<ProcEvent>,
    input: String,
    focus: Focus,
    output_scroll: u16,
    output_view_lines: usize,
    /// Exact wrapped-row count of the CLI Output `Paragraph` at its current
    /// width (`Paragraph::line_count`, ratatui's own wrap calculation - not
    /// an approximation). `output_scroll` is a *row* offset once wrapping is
    /// enabled (`Wrap { trim: false }`), not a log-entry offset, so
    /// `max_output_scroll` must be computed from this, not `logs.len()` -
    /// using entry count there was the original bug: any entry that wraps
    /// into more than one row (long docker command lines are common) made
    /// the "scroll to bottom" target too small, so following new output
    /// would visibly stop short of the true tail until something else
    /// (manual scroll, resize) happened to close the gap.
    output_total_rows: usize,
    follow_output: bool,
    last_log_count: usize,
    docker_panel: DockerPanelData,
    log_file: Option<std::fs::File>,
    selected_model: Option<String>,
    model_items: Vec<String>,
    input_purpose: InputPurpose,
    last_docker_refresh: Instant,
    /// The dashboard list's last-rendered screen area, cached so mouse clicks
    /// can be hit-tested against it without re-computing the layout.
    last_dashboard_area: Rect,
    /// Selected row in the Running panel (`Focus::Running`).
    running_selected: usize,
    /// Sends chat turns to the persistent background worker (see
    /// `spawn_chat_worker`); replies come back on `rx` as `ProcEvent::ChatReply`.
    chat_tx: Sender<ChatRequest>,
    /// Loopback port the embedded MCP HTTP server listens on (see
    /// `spawn_mcp_server`); shown in the "MCP Server" panel.
    mcp_port: u16,
    /// Approval requests not yet drained into `input_purpose` (only one is
    /// shown at a time - see `pump_mcp_approvals`).
    mcp_pending: VecDeque<PendingApproval>,
    /// The reply channel for the approval currently shown in the input box
    /// (`InputPurpose::ApproveMcp`), answered by `send_input_line`.
    active_mcp_respond: Option<oneshot::Sender<bool>>,
    mcp_approval_rx: UnboundedReceiver<PendingApproval>,
    /// `docker ps` runs on a background thread (see `refresh_docker_panel`)
    /// so a slow/hung Docker daemon can never stall the render loop; results
    /// come back here and are applied by `pump_docker_panel`.
    docker_panel_rx: Receiver<DockerPanelData>,
    docker_panel_tx: Sender<DockerPanelData>,
    docker_refresh_in_flight: Arc<AtomicBool>,
}

impl AppState {
    fn new(repo_root: Option<std::path::PathBuf>, loaded: LoadedConfig) -> Self {
        const MCP_PORT: u16 = 7332;

        let (tx, rx) = mpsc::channel::<ProcEvent>();
        let (mcp_approval, mcp_approval_rx) = ApprovalGate::new();
        spawn_mcp_server(loaded.clone(), mcp_approval.clone(), MCP_PORT, tx.clone());
        let chat_tx = spawn_chat_worker(loaded, mcp_approval, tx.clone());
        let (docker_panel_tx, docker_panel_rx) = mpsc::channel::<DockerPanelData>();

        let log_file = open_session_log(&repo_root);

        let mut app = Self {
            menu: MenuKind::Root,
            commands: menu_items(MenuKind::Root),
            selected: 0,
            repo_root,
            logs: vec![],
            running: Vec::new(),
            tx,
            rx,
            input: String::new(),
            focus: Focus::Dashboard,
            output_scroll: 0,
            output_view_lines: 0,
            output_total_rows: 0,
            follow_output: true,
            last_log_count: 1,
            docker_panel: DockerPanelData {
                available: false,
                rows: Vec::new(),
            },
            log_file,
            selected_model: None,
            model_items: Vec::new(),
            input_purpose: InputPurpose::None,
            last_docker_refresh: Instant::now(),
            last_dashboard_area: Rect::default(),
            running_selected: 0,
            chat_tx,
            mcp_port: MCP_PORT,
            mcp_pending: VecDeque::new(),
            active_mcp_respond: None,
            mcp_approval_rx,
            docker_panel_rx,
            docker_panel_tx,
            docker_refresh_in_flight: Arc::new(AtomicBool::new(false)),
        };

        app.refresh_ui();
        app
    }

    /// Append a single log entry to the on-disk session log, prefixed with a
    /// local timestamp. Commands get a prominent marker so a session reads as a
    /// dated history of what was run and what it printed. Mirrors what the
    /// "CLI Output" panel shows; failures to write are intentionally silent so
    /// logging never disrupts the TUI.
    fn write_log_line(&mut self, entry: &LogEntry) {
        let Some(file) = self.log_file.as_mut() else {
            return;
        };

        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = match entry {
            LogEntry::Command(text) => format!("[{}] >>> COMMAND: {}", ts, text),
            LogEntry::Info(text) => format!("[{}] INFO  | {}", ts, text),
            LogEntry::Error(text) => format!("[{}] ERROR | {}", ts, text),
            LogEntry::Stdout(text) => format!("[{}] OUT   | {}", ts, text),
            LogEntry::Stderr(text) => format!("[{}] ERR   | {}", ts, text),
            LogEntry::Mcp(text) => format!("[{}] MCP   | {}", ts, text),
        };

        let _ = writeln!(file, "{}", line);
        let _ = file.flush();
    }

    /// Record one log entry: persist it to the session log and show it in the
    /// "CLI Output" panel. Replaces direct pushes to `self.logs` so every line
    /// is logged in exactly one place.
    fn push_log(&mut self, entry: LogEntry) {
        self.write_log_line(&entry);
        self.logs.push(entry);
    }

    /// Record many log entries (used for streamed stdout/stderr chunks).
    fn extend_log<I: IntoIterator<Item = LogEntry>>(&mut self, entries: I) {
        for entry in entries {
            self.push_log(entry);
        }
    }

    fn refresh_ui(&mut self) {
        self.refresh_docker_panel();
    }

    /// Kick off a `docker ps` refresh on a background thread; never blocks
    /// the render loop, unlike calling `docker` directly here would (a slow
    /// or hung Docker daemon must not stall the whole TUI). Results are
    /// applied later by `pump_docker_panel`. A no-op if a refresh from
    /// startup, the periodic timer, or a manual `r` press is already running.
    fn refresh_docker_panel(&mut self) {
        if self.docker_refresh_in_flight.swap(true, Ordering::SeqCst) {
            return;
        }
        let tx = self.docker_panel_tx.clone();
        let in_flight = self.docker_refresh_in_flight.clone();
        thread::spawn(move || {
            let _ = tx.send(fetch_docker_panel_data());
            in_flight.store(false, Ordering::SeqCst);
        });
    }

    /// Apply the latest background `docker ps` result, if one has arrived.
    fn pump_docker_panel(&mut self) {
        while let Ok(data) = self.docker_panel_rx.try_recv() {
            self.docker_panel = data;
        }
    }

    fn max_output_scroll(&self) -> u16 {
        self.output_total_rows
            .saturating_sub(self.output_view_lines)
            .min(u16::MAX as usize) as u16
    }

    fn output_page_up(&mut self) {
        let step = self.output_view_lines.max(1).min(u16::MAX as usize) as u16;
        self.follow_output = false;
        self.output_scroll = self.output_scroll.saturating_sub(step);
    }

    fn output_page_down(&mut self) {
        let step = self.output_view_lines.max(1).min(u16::MAX as usize) as u16;
        let max_scroll = self.max_output_scroll();
        self.output_scroll = self.output_scroll.saturating_add(step).min(max_scroll);
        if self.output_scroll >= max_scroll {
            self.follow_output = true;
            self.output_scroll = max_scroll;
        }
    }

    /// Scroll the CLI Output panel by a single line (mouse wheel granularity),
    /// as opposed to `output_page_up`/`down`'s full-page jump.
    fn output_line_up(&mut self) {
        self.follow_output = false;
        self.output_scroll = self.output_scroll.saturating_sub(1);
    }

    fn output_line_down(&mut self) {
        let max_scroll = self.max_output_scroll();
        self.output_scroll = self.output_scroll.saturating_add(1).min(max_scroll);
        if self.output_scroll >= max_scroll {
            self.follow_output = true;
            self.output_scroll = max_scroll;
        }
    }

    fn set_menu(&mut self, menu: MenuKind) {
        self.menu = menu;
        // The model-selection menu is built dynamically from `model_items`;
        // every other menu uses the static menu tree.
        self.commands = if menu == MenuKind::SelectModel {
            Vec::new()
        } else {
            menu_items(menu)
        };
        self.selected = 0;
    }

    /// Number of selectable rows in the current menu. The model menu shows at
    /// least one row (a placeholder when no models are installed).
    fn item_count(&self) -> usize {
        if self.menu == MenuKind::SelectModel {
            self.model_items.len().max(1)
        } else {
            self.commands.len()
        }
    }

    /// The AI submenu to return to after the model menu, per host OS.
    fn ai_back_menu(&self) -> MenuKind {
        if cfg!(windows) {
            MenuKind::RunWin11Ai
        } else {
            MenuKind::RunUbuntu22Ai
        }
    }

    /// The menu one level up from the current one, mirroring the targets of
    /// each menu's own "< Back to ..." item. `None` means the current menu is
    /// the Dashboard root, so there is nowhere further to go back to.
    fn parent_menu(&self) -> Option<MenuKind> {
        match self.menu {
            MenuKind::Root => None,
            MenuKind::RunUbuntu22 | MenuKind::RunWin11 => Some(MenuKind::Root),
            MenuKind::RunUbuntu22Network
            | MenuKind::RunUbuntu22Dependencies
            | MenuKind::RunUbuntu22Github
            | MenuKind::RunUbuntu22Open
            | MenuKind::RunUbuntu22Ai
            | MenuKind::RunUbuntu22Dotfiles => Some(MenuKind::RunUbuntu22),
            MenuKind::RunWin11Network
            | MenuKind::RunWin11Dependencies
            | MenuKind::RunWin11Github
            | MenuKind::RunWin11Open
            | MenuKind::RunWin11Ai
            | MenuKind::RunWin11Dotfiles => Some(MenuKind::RunWin11),
            MenuKind::SelectModel => Some(self.ai_back_menu()),
        }
    }

    /// Run `vn ai models` and load the printed model names into `model_items`.
    /// Blocks briefly; if Ollama is down the command fails fast and the list
    /// stays empty.
    fn load_models_into_menu(&mut self) {
        self.model_items.clear();

        let exe = match env::current_exe() {
            Ok(path) => path,
            Err(_) => return,
        };

        let mut cmd = Command::new(exe);
        if let Some(repo_root) = &self.repo_root {
            cmd.arg("--repo-root").arg(repo_root);
        }
        // Chat always attaches tools now, so only offer models that actually
        // support tool-calling (see commands::ai::model_supports_tools).
        cmd.args(["ai", "models", "--tools-only"])
            .stdin(Stdio::null());

        if let Ok(output) = cmd.output() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let name = line.trim();
                if !name.is_empty() {
                    self.model_items.push(name.to_string());
                }
            }
        }
    }

    fn next(&mut self) {
        let count = self.item_count();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    fn previous(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        if self.selected == 0 {
            self.selected = count - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn running_next(&mut self) {
        if !self.running.is_empty() {
            self.running_selected = (self.running_selected + 1) % self.running.len();
        }
    }

    fn running_previous(&mut self) {
        if self.running.is_empty() {
            return;
        }
        if self.running_selected == 0 {
            self.running_selected = self.running.len() - 1;
        } else {
            self.running_selected -= 1;
        }
    }

    /// Kill just the selected process in the Running panel (as opposed to
    /// `kill_all_running`, which stops everything).
    fn kill_selected_running(&mut self) {
        if self.running.is_empty() {
            self.push_log(LogEntry::Info(
                "[INFO] No running processes to stop.".to_string(),
            ));
            self.trim_logs();
            return;
        }
        if self.running_selected >= self.running.len() {
            self.running_selected = self.running.len() - 1;
        }
        let mut proc = self.running.remove(self.running_selected);
        let _ = proc.child.kill();
        if self.running_selected >= self.running.len() && self.running_selected > 0 {
            self.running_selected -= 1;
        }
        self.push_log(LogEntry::Info(format!("[INFO] Stopped '{}'.", proc.label)));
        self.trim_logs();
    }

    /// Handle a left-click at terminal coordinates `(col, row)`: if it lands
    /// on a dashboard row, select and run it (mirrors Up/Down + Enter). Safe
    /// to one-click even for destructive items since those now require
    /// typing "yes" to confirm.
    fn click_dashboard_row(&mut self, col: u16, row: u16) {
        let area = self.last_dashboard_area;
        if area.width < 2 || area.height < 2 {
            return;
        }
        let inner_x0 = area.x + 1;
        let inner_x1 = area.x + area.width - 1;
        let inner_y0 = area.y + 1;
        let inner_y1 = area.y + area.height - 1;
        if col < inner_x0 || col >= inner_x1 || row < inner_y0 || row >= inner_y1 {
            return;
        }

        let index = (row - inner_y0) as usize;
        if index >= self.item_count() {
            return;
        }

        self.focus = Focus::Dashboard;
        self.selected = index;
        self.activate_selected();
    }

    fn activate_selected(&mut self) {
        // The model-selection menu is dynamic and has no CommandItem entries;
        // handle picking a model (or the empty placeholder) up front.
        if self.menu == MenuKind::SelectModel {
            let back = self.ai_back_menu();
            if self.model_items.is_empty() {
                self.push_log(LogEntry::Info(
                    "[INFO] No models installed. Use Download Model to fetch one.".to_string(),
                ));
                self.set_menu(back);
                self.trim_logs();
                return;
            }
            let name = self.model_items[self.selected].clone();
            // Only a genuine switch mid-session should reset history - not
            // the first selection in a fresh TUI run, which would otherwise
            // needlessly discard a legitimate resumed conversation with the
            // same model from a previous run (there's no prior in-run
            // selection to compare against yet, so `selected_model` is still
            // `None` at that point).
            let switched = matches!(&self.selected_model, Some(prev) if prev != &name);
            self.selected_model = Some(name.clone());
            self.push_log(LogEntry::Command(format!("select model: {}", name)));
            self.push_log(LogEntry::Info(format!(
                "[INFO] Active model set to '{}'.",
                name
            )));
            // A different model shouldn't inherit the previous model's chat
            // history - it has no way to know which of those prior
            // "assistant" turns were grounded in a real tool call versus
            // fabricated, and tends to treat them as established fact rather
            // than re-checking. No-op if the same model was reselected.
            if switched && self.chat_tx.send(ChatRequest::ResetSession).is_ok() {
                self.push_log(LogEntry::Info(
                    "[INFO] Started a new chat session (previous conversation history is not \
                     carried over to a different model)."
                        .to_string(),
                ));
            }
            self.set_menu(back);
            self.trim_logs();
            return;
        }

        if self.commands.is_empty() {
            return;
        }

        let item = self.commands[self.selected].clone();

        match item.action {
            Action::OpenMenu(next_menu) => {
                if !menu_allowed_on_current_os(next_menu) {
                    self.push_log(LogEntry::Command(item.label.to_string()));
                    self.push_log(LogEntry::Error(
                        "[WARNING] This submenu is not supported on the current OS.".to_string(),
                    ));
                    if cfg!(windows) {
                        self.push_log(LogEntry::Info(
                            "[INFO] Windows host: use vn run win11 submenu items.".to_string(),
                        ));
                    } else {
                        self.push_log(LogEntry::Info(
                            "[INFO] Non-Windows host: use vn run ubuntu22 submenu items."
                                .to_string(),
                        ));
                    }
                    self.trim_logs();
                    return;
                }

                self.push_log(LogEntry::Command(item.label.to_string()));
                self.push_log(LogEntry::Info("[INFO] Opened submenu.".to_string()));
                self.set_menu(next_menu);
                self.trim_logs();
            }
            Action::BackToRoot => {
                self.push_log(LogEntry::Command(item.label.to_string()));
                self.push_log(LogEntry::Info("[INFO] Returned to Dashboard.".to_string()));
                self.set_menu(MenuKind::Root);
                self.trim_logs();
            }
            Action::Execute(args) => {
                let args = args.into_iter().map(String::from).collect();
                self.spawn_process(item.label, args);
            }
            Action::ExecuteConfirm(args) => {
                self.push_log(LogEntry::Command(item.label.to_string()));
                self.push_log(LogEntry::Error(format!(
                    "[WARNING] This will run '{}'. Type 'yes' and press Enter to confirm, or Tab to cancel.",
                    item.label
                )));
                self.input_purpose = InputPurpose::ConfirmDestructive(args, item.label);
                self.focus = Focus::Input;
                self.input.clear();
                self.trim_logs();
            }
            Action::OpenModelMenu => {
                self.push_log(LogEntry::Command(item.label.to_string()));
                self.push_log(LogEntry::Info(
                    "[INFO] Loading installed models...".to_string(),
                ));
                self.load_models_into_menu();
                if self.model_items.is_empty() {
                    self.push_log(LogEntry::Info(
                        "[INFO] No tool-capable models found. Chat always uses tools, so models \
                         without that capability (e.g. gemma3:1b) aren't listed here. Is Ollama \
                         running? Try Open Ollama, or Download Model with 'llama3.2' (small, supports tools)."
                            .to_string(),
                    ));
                } else {
                    self.push_log(LogEntry::Info(format!(
                        "[INFO] {} model(s) found. Select one and press Enter.",
                        self.model_items.len()
                    )));
                }
                self.set_menu(MenuKind::SelectModel);
                self.trim_logs();
            }
            Action::ArmInput(purpose) => {
                self.focus = Focus::Input;
                self.input.clear();
                self.push_log(LogEntry::Command(item.label.to_string()));
                match &purpose {
                    InputPurpose::DownloadModel => self.push_log(LogEntry::Info(
                        "[INFO] Type a model name (e.g. llama3.2) and press Enter to download. Tab returns to the dashboard."
                            .to_string(),
                    )),
                    InputPurpose::Chat => {
                        let model = self
                            .selected_model
                            .clone()
                            .unwrap_or_else(|| "default".to_string());
                        self.push_log(LogEntry::Info(format!(
                            "[INFO] Chatting with '{}'. Type a message and press Enter. Tab returns to the dashboard.",
                            model
                        )));
                    }
                    InputPurpose::None
                    | InputPurpose::ConfirmDestructive(..)
                    | InputPurpose::ApproveMcp(_) => {}
                }
                self.input_purpose = purpose;
                self.trim_logs();
            }
        }
    }

    fn spawn_process(&mut self, label: &str, args: Vec<String>) {
        self.push_log(LogEntry::Command(label.to_string()));

        let exe = match env::current_exe() {
            Ok(path) => path,
            Err(err) => {
                self.push_log(LogEntry::Error(format!(
                    "[ERROR] Could not resolve current executable: {}",
                    err
                )));
                self.trim_logs();
                return;
            }
        };

        let mut cmd = Command::new(exe);
        if let Some(repo_root) = &self.repo_root {
            cmd.arg("--repo-root").arg(repo_root);
            cmd.current_dir(repo_root);
            cmd.env("VNCLI_REPO_ROOT", repo_root);
        }

        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = match cmd.group_spawn() {
            Ok(c) => c,
            Err(err) => {
                self.push_log(LogEntry::Error(format!(
                    "[ERROR] Failed to start command: {}",
                    err
                )));
                self.trim_logs();
                return;
            }
        };

        let stdout = child.inner().stdout.take();
        let stderr = child.inner().stderr.take();

        if let Some(mut out) = stdout {
            let tx_out = self.tx.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match out.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf[..n]).to_string();
                            if tx_out.send(ProcEvent::Stdout(text)).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        if let Some(mut err) = stderr {
            let tx_err = self.tx.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match err.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf[..n]).to_string();
                            if tx_err.send(ProcEvent::Stderr(text)).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        self.push_log(LogEntry::Info(
            "[INFO] Process started in background.".to_string(),
        ));
        self.running.push(RunningProcess {
            label: label.to_string(),
            child,
            started_at: Instant::now(),
        });
        self.trim_logs();
    }

    fn send_input_line(&mut self) {
        let text = self.input.trim().to_string();
        self.input.clear();
        if text.is_empty() {
            return;
        }

        match self.input_purpose.clone() {
            InputPurpose::DownloadModel => {
                self.spawn_process(
                    "vn ai pull",
                    vec!["ai".to_string(), "pull".to_string(), text],
                );
                // One-shot: return focus to the dashboard after starting.
                self.input_purpose = InputPurpose::None;
                self.focus = Focus::Dashboard;
            }
            InputPurpose::Chat => {
                let model = self.selected_model.clone();
                self.push_log(LogEntry::Command(format!("You: {}", text)));
                let request = ChatRequest::Turn {
                    model,
                    message: text,
                };
                if self.chat_tx.send(request).is_err() {
                    self.push_log(LogEntry::Error(
                        "[ERROR] Chat worker is not available.".to_string(),
                    ));
                } else {
                    self.push_log(LogEntry::Info("[INFO] Waiting for reply...".to_string()));
                }
                // Stay in chat mode so the conversation can continue.
            }
            InputPurpose::ConfirmDestructive(args, label) => {
                if text.eq_ignore_ascii_case("yes") {
                    self.spawn_process(label, args.into_iter().map(String::from).collect());
                } else {
                    self.push_log(LogEntry::Info(format!("[INFO] Cancelled: {}", label)));
                }
                self.input_purpose = InputPurpose::None;
                self.focus = Focus::Dashboard;
            }
            InputPurpose::ApproveMcp(description) => {
                let approved = text.eq_ignore_ascii_case("yes");
                if let Some(respond) = self.active_mcp_respond.take() {
                    let _ = respond.send(approved);
                }
                self.push_log(LogEntry::Info(format!(
                    "[MCP] {}: {}",
                    if approved { "Approved" } else { "Denied" },
                    description
                )));
                self.input_purpose = InputPurpose::None;
                self.focus = Focus::Dashboard;
            }
            InputPurpose::None => {
                self.push_log(LogEntry::Info(
                    "[INFO] Input is not attached to an action. Use Download Model or Chat first."
                        .to_string(),
                ));
            }
        }

        self.trim_logs();
    }

    fn pump_process(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                ProcEvent::Stdout(chunk) => self.extend_log(
                    split_to_entries(chunk, false)
                        .into_iter()
                        .map(LogEntry::Stdout),
                ),
                ProcEvent::Stderr(chunk) => self.extend_log(
                    split_to_entries(chunk, true)
                        .into_iter()
                        .map(LogEntry::Stderr),
                ),
                // A `Line`/`Span` renders as one visual line regardless of
                // embedded '\n's, so multi-line text (a markdown-formatted
                // chat reply, a multi-line tool result) must be split into
                // one LogEntry per physical line here, same as process
                // stdout/stderr chunks - otherwise it shows as one squashed
                // line even with wrapping enabled.
                ProcEvent::ChatReply(Ok(text)) => self.extend_log(
                    split_to_entries(text, false)
                        .into_iter()
                        .map(LogEntry::Stdout),
                ),
                ProcEvent::ChatReply(Err(err)) => {
                    self.push_log(LogEntry::Error(format!("[ERROR] {}", err)))
                }
                ProcEvent::McpActivity(text) => {
                    self.extend_log(split_to_entries(text, false).into_iter().map(LogEntry::Mcp))
                }
            }
        }

        let mut idx = 0;
        while idx < self.running.len() {
            let (remove_current, message) = {
                let proc = &mut self.running[idx];
                match proc.child.try_wait() {
                    Ok(Some(status)) => (
                        true,
                        Some(LogEntry::Info(format!(
                            "[INFO] Process '{}' exited with status: {}",
                            proc.label, status
                        ))),
                    ),
                    Ok(None) => (false, None),
                    Err(err) => (
                        true,
                        Some(LogEntry::Error(format!(
                            "[ERROR] Failed checking process '{}' status: {}",
                            proc.label, err
                        ))),
                    ),
                }
            };

            if let Some(message) = message {
                self.push_log(message);
            }

            if remove_current {
                self.running.remove(idx);
            } else {
                idx += 1;
            }
        }

        self.trim_logs();
    }

    /// Drain newly-arrived MCP approval requests (from the embedded server or
    /// the Ollama chat's tool-calling loop) into `mcp_pending`, then, if
    /// nothing is already being shown, arm the input box with the next one.
    /// A pending approval takes over the input box regardless of what it was
    /// previously armed for - these are blocking requests from the caller's
    /// point of view, so they can't wait for you to finish an unrelated typed
    /// line first.
    fn pump_mcp_approvals(&mut self) {
        while let Ok(pending) = self.mcp_approval_rx.try_recv() {
            self.mcp_pending.push_back(pending);
        }

        if !matches!(self.input_purpose, InputPurpose::ApproveMcp(_)) {
            if let Some(pending) = self.mcp_pending.pop_front() {
                self.push_log(LogEntry::Error(format!(
                    "[MCP] Approval requested: {}. Type 'yes' to approve, anything else to deny.",
                    pending.description
                )));
                self.active_mcp_respond = Some(pending.respond);
                self.input_purpose = InputPurpose::ApproveMcp(pending.description);
                self.focus = Focus::Input;
                self.input.clear();
                self.trim_logs();
            }
        }
    }

    fn shutdown(&mut self) {
        for proc in &mut self.running {
            let _ = proc.child.kill();
        }
        self.running.clear();
    }

    /// Kill every currently running background process without exiting the
    /// TUI (e.g. to cancel a stuck `docker build` or `yt-dlp` job).
    fn kill_all_running(&mut self) {
        let count = self.running.len();
        if count == 0 {
            self.push_log(LogEntry::Info(
                "[INFO] No running processes to stop.".to_string(),
            ));
            self.trim_logs();
            return;
        }
        for proc in &mut self.running {
            let _ = proc.child.kill();
        }
        self.running.clear();
        self.push_log(LogEntry::Info(format!(
            "[INFO] Stopped {} running process(es).",
            count
        )));
        self.trim_logs();
    }

    /// Scroll the CLI Output panel up to the nearest Error/Stderr entry above
    /// the current view. Repeated presses walk further back through earlier
    /// errors; cheaper than a full search box for "did the last command fail".
    /// Treats `output_scroll` (a wrapped-*row* offset, see
    /// `AppState::output_total_rows`) as an *entry* index into `self.logs` -
    /// an approximation, since mapping a row offset back to the entry index
    /// it falls within would need re-wrapping every entry above it. Only
    /// affects how far back one keypress jumps, not whether output is
    /// visible/current, so left as-is rather than adding that cost here.
    fn jump_to_previous_error(&mut self) {
        if self.logs.is_empty() {
            return;
        }
        let search_end = (self.output_scroll as usize).min(self.logs.len());
        let found = self.logs[..search_end]
            .iter()
            .rposition(|entry| matches!(entry, LogEntry::Error(_) | LogEntry::Stderr(_)));

        let Some(idx) = found else {
            self.push_log(LogEntry::Info(
                "[INFO] No earlier errors found.".to_string(),
            ));
            self.trim_logs();
            return;
        };

        self.follow_output = false;
        let above = (self.output_view_lines.saturating_sub(1)) as u16;
        self.output_scroll = (idx as u16).saturating_sub(above);
        self.clamp_output_scroll();
    }

    fn trim_logs(&mut self) {
        if self.logs.len() > MAX_LOG_ENTRIES {
            let overflow = self.logs.len() - MAX_LOG_ENTRIES;
            self.logs.drain(0..overflow);
        }

        if self.logs.len() != self.last_log_count {
            self.last_log_count = self.logs.len();
            self.follow_output = true;
        }

        self.clamp_output_scroll();
    }

    /// Keep `output_scroll` pinned to the bottom when following, or clamped to
    /// the valid range otherwise. Shared by `trim_logs()` and the render loop
    /// (whose `output_view_lines` changes with terminal size).
    fn clamp_output_scroll(&mut self) {
        let max_scroll = self.max_output_scroll();
        self.output_scroll = if self.follow_output {
            max_scroll
        } else {
            self.output_scroll.min(max_scroll)
        };
    }
}

/// Open (creating if needed) the persistent TUI session log under
/// `<repo_root>/logs/vn-tui.log` in append mode and write a session header.
/// Falls back to the current directory if no repo root is known. The `logs/`
/// directory and `*.log` files are gitignored, so this never reaches GitHub.
/// Returns `None` if the log cannot be opened; the TUI then runs without
/// file logging rather than failing.
fn open_session_log(repo_root: &Option<std::path::PathBuf>) -> Option<std::fs::File> {
    let base = repo_root.clone().or_else(|| env::current_dir().ok())?;
    let dir = base.join("logs");
    std::fs::create_dir_all(&dir).ok()?;

    let path = dir.join("vn-tui.log");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;

    let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(file);
    let _ = writeln!(file, "[{}] ===== vn TUI session started =====", ts);
    let _ = file.flush();

    Some(file)
}

/// Spawn the embedded MCP HTTP server (loopback-only) on its own thread/tokio
/// runtime, matching `spawn_chat_worker`'s style. Destructive tool calls
/// (`stop_app`) go through `approval`, which the TUI drains every frame (see
/// `pump_mcp_approvals`) - unlike the standalone `vn mcp serve` CLI command,
/// this one has a TUI attached, so approvals actually get a prompt instead of
/// being auto-denied. Bind/serve errors are reported into the CLI Output
/// panel via `tx` rather than printed directly (which would corrupt the
/// ratatui display).
fn spawn_mcp_server(
    loaded: LoadedConfig,
    approval: ApprovalGate,
    port: u16,
    tx: Sender<ProcEvent>,
) {
    thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                let _ = tx.send(ProcEvent::Stderr(format!(
                    "[MCP] failed to start server runtime: {err}\n"
                )));
                return;
            }
        };
        if let Err(err) = rt.block_on(mcp_command::serve_http(loaded, port, approval, true)) {
            let _ = tx.send(ProcEvent::Stderr(format!("[MCP] server error: {err:#}\n")));
        }
    });
}

/// Spawn a persistent background worker that owns the Ollama client and chat
/// session for the TUI's Chat feature. Replaces the old design of re-spawning
/// `vn ai chat` as a subprocess per message: that re-created the Ollama client
/// and re-read/re-parsed the whole session file from disk on every turn. This
/// worker builds the client and loads the session once, then keeps both in
/// memory for the life of the TUI, appending to (and saving) the session file
/// after each reply. Returns the `Sender` used to submit chat turns; replies
/// come back on `tx` as `ProcEvent::ChatReply`.
/// Tool-calling rounds per chat turn: the model can call a tool, see the
/// result, and call another before replying, but this bounds how many times
/// that can chain so a confused model can't loop forever.
const MAX_TOOL_ROUNDS: u32 = 4;

/// Bounds a single Ollama chat completion call. `ollama-rs`'s HTTP client has
/// no timeout of its own, so a hung/stuck local model (or a confused one
/// stuck generating a runaway reply after e.g. a hallucinated tool call
/// error) would otherwise block `spawn_chat_worker`'s single worker thread
/// forever - no crash, no error, just the CLI Output panel going silent
/// after "[INFO] Waiting for reply..." while the rest of the TUI (a
/// different thread) stays responsive, which is exactly what makes this look
/// like "the CLI output stopped" rather than an obvious hang. Generous on
/// purpose: real replies with tool calls can legitimately take a while on
/// slow hardware.
const CHAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Sampling temperature for chat completions. Lower than Ollama's per-model
/// default (commonly ~0.7-0.8) specifically to make tool-calling decisions
/// more consistent - empirically (see `TOOL_USE_REMINDER`'s doc), even a
/// model fine-tuned for tool use skips calling one for a state question a
/// meaningful fraction of the time at default temperature, especially once
/// prior conversation turns are in context; a lower temperature doesn't
/// eliminate that on its own, but cuts run-to-run variance enough that the
/// reminder below reliably lands instead of being a coin flip.
const CHAT_TEMPERATURE: f32 = 0.2;

/// Appended to the outgoing copy of the latest user message only (not to
/// what's displayed or saved to the session - see its use in the request
/// loop) to counter an observed failure mode: a local model, even one
/// fine-tuned for tool use (`llama3-groq-tool-use`), reliably stops calling
/// tools for state questions ("list the containers") once even one prior
/// assistant turn with no tool call is already in its context - confirmed by
/// hand-crafting the exact request Ollama's `/api/chat` receives and
/// replaying it directly: identical multi-turn history with vs. without this
/// reminder line was the difference between 0/3 and 5/5 tool calls across
/// repeated runs. Cheaper and more reliable than trying to prevent
/// untool-called replies from ever entering history (MCP has no
/// `tool_choice: required` equivalent to force this outright - checked, not
/// supported by this Ollama version).
const TOOL_USE_REMINDER: &str =
    "\n\n(Answer by calling the matching tool now - do not answer from memory or a previous turn.)";

/// Cap on how many *saved* messages (`session.messages`, persisted across
/// app restarts with no automatic expiry) are replayed into a chat request -
/// independent of `MAX_LOG_ENTRIES`, which only bounds the CLI Output
/// display. A long-lived session accumulates without limit (108 messages
/// deep was observed in practice after a day of testing); the more of that
/// accumulates as "assistant" turns that didn't call a tool, the harder
/// `TOOL_USE_REMINDER`/`CHAT_TEMPERATURE` have to fight just to get back to
/// the reliability a fresh conversation already has for free. This trims
/// what's *sent*, not what's *saved* - `session.messages` itself, and the
/// full CLI Output/log file, are unaffected.
const MAX_HISTORY_MESSAGES: usize = 12;

/// Build the Ollama tool definitions for every tool `toolset` exposes, from
/// the same JSON-schema metadata an MCP client would see via `tools/list`.
/// `ollama-rs`'s `ToolInfo`/`ToolFunctionInfo` are plain public structs, so
/// this works for a runtime-discovered tool set with no compile-time-known
/// `Tool` types on the ollama-rs side.
fn build_ollama_tools(toolset: &AppsToolset) -> Vec<ToolInfo> {
    toolset
        .list_tools()
        .into_iter()
        .filter_map(|tool| {
            let schema_value = serde_json::Value::Object((*tool.input_schema).clone());
            let parameters: schemars::Schema = serde_json::from_value(schema_value).ok()?;
            Some(ToolInfo {
                tool_type: ToolType::Function,
                function: ToolFunctionInfo {
                    name: tool.name.to_string(),
                    description: tool.description.clone().unwrap_or_default().to_string(),
                    parameters,
                },
            })
        })
        .collect()
}

/// Extract the text content of an MCP `CallToolResult` for display and for
/// feeding back to the model as a `tool`-role message.
fn call_tool_result_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn spawn_chat_worker(
    loaded: LoadedConfig,
    mcp_approval: ApprovalGate,
    tx: Sender<ProcEvent>,
) -> Sender<ChatRequest> {
    let (req_tx, req_rx) = mpsc::channel::<ChatRequest>();

    thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                let _ = tx.send(ProcEvent::ChatReply(Err(format!(
                    "failed to start chat runtime: {err}"
                ))));
                return;
            }
        };

        let sessions_dir = expand_tilde(&loaded.config.sessions.dir);
        let path = session_path(&sessions_dir, "tui");
        let mut session = match SessionFile::load_or_new(&path, "tui") {
            Ok(s) => s,
            Err(err) => {
                let _ = tx.send(ProcEvent::ChatReply(Err(format!(
                    "failed to load chat session: {err}"
                ))));
                return;
            }
        };

        let ollama = crate::commands::ai::build_client(&loaded.config.ollama.host);
        let default_model = loaded.config.ollama.model.clone();
        let system_prompt = loaded.config.prompts.system.clone();
        // Same tool implementations the MCP server exposes, called in-process
        // here (no HTTP/stdio round-trip needed since we're in the same
        // process) - `stop_app` still goes through `mcp_approval`, so a
        // destructive tool call the model makes surfaces the same TUI
        // approval prompt as one from an external MCP client. `false` here
        // (unlike the headless/external server) since this is the user's own
        // interactive chat: open_app should actually pop the browser once the
        // app is ready, matching what "open the doc processor" reads as.
        let toolset = AppsToolset::new(loaded, mcp_approval, false);
        let tools = build_ollama_tools(&toolset);
        // Forwards each progress line a report-based tool call produces (e.g.
        // a docker build's output) straight to the CLI Output panel as it
        // happens, instead of the panel sitting idle for the whole call and
        // then dumping everything at once when `call_by_name` returns.
        // `live_line_count` tracks whether *this* call streamed anything, so
        // the "[MCP] Result: ..." line after it doesn't repeat the same
        // dozens of lines a second time in full - a docker build otherwise
        // shows up twice back to back in the CLI Output, once live and once
        // as one large dump.
        let live_line_count = Arc::new(AtomicUsize::new(0));
        let tool_live: crate::mcp::LiveReporter = {
            let tx = tx.clone();
            let live_line_count = live_line_count.clone();
            Arc::new(move |line: &str| {
                live_line_count.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(ProcEvent::McpActivity(line.to_string()));
            })
        };

        while let Ok(req) = req_rx.recv() {
            let (model, req_message) = match req {
                ChatRequest::ResetSession => {
                    session.messages.clear();
                    if let Err(err) = session.save(&path) {
                        let _ = tx.send(ProcEvent::McpActivity(format!(
                            "[INFO] Failed to persist the cleared chat session: {err:#}"
                        )));
                    }
                    continue;
                }
                ChatRequest::Turn { model, message } => (model, message),
            };
            let model = model.unwrap_or_else(|| default_model.clone());

            let mut messages: Vec<ChatMessage> = Vec::new();
            if let Some(system) = &system_prompt {
                messages.push(ChatMessage::system(system.clone()));
            }
            let history_start = session.messages.len().saturating_sub(MAX_HISTORY_MESSAGES);
            for msg in &session.messages[history_start..] {
                match msg.role.as_str() {
                    "user" => messages.push(ChatMessage::user(msg.content.clone())),
                    "assistant" => messages.push(ChatMessage::assistant(msg.content.clone())),
                    "system" => messages.push(ChatMessage::system(msg.content.clone())),
                    _ => {}
                }
            }
            // The reminder is only in the copy sent to the model - `req_message`
            // (used below for the CLI Output panel and the saved session) stays
            // exactly what the user typed.
            messages.push(ChatMessage::user(format!(
                "{req_message}{TOOL_USE_REMINDER}"
            )));

            // Ground truth for whether this reply is backed by an actual tool
            // call, or is just the model talking - a small local model will
            // sometimes claim to have done something (e.g. "restarted the
            // container!") without calling any tool. Set the moment any tool
            // call is dispatched below; checked once the turn finishes to tag
            // the reply if it's still false. See `[MCP: NONE]` below.
            let mut tool_called = false;

            let result: std::result::Result<String, ollama_rs::error::OllamaError> =
                rt.block_on(async {
                    for _ in 0..MAX_TOOL_ROUNDS {
                        let request = ChatMessageRequest::new(model.clone(), messages.clone())
                            .tools(tools.clone())
                            .options(ModelOptions::default().temperature(CHAT_TEMPERATURE));
                        let response = tokio::time::timeout(
                            CHAT_REQUEST_TIMEOUT,
                            ollama.send_chat_messages(request),
                        )
                        .await
                        .unwrap_or_else(|_elapsed| {
                            Err(ollama_rs::error::OllamaError::Other(format!(
                                "chat request to Ollama timed out after {}s - the model may be \
                                 hung/stuck generating, or Ollama itself may be unresponsive; \
                                 try again, or restart Ollama if this keeps happening",
                                CHAT_REQUEST_TIMEOUT.as_secs()
                            )))
                        })?;
                        let msg = response.message;

                        if msg.tool_calls.is_empty() {
                            return Ok(msg.content);
                        }

                        messages.push(msg.clone());
                        for call in &msg.tool_calls {
                            tool_called = true;
                            let name = call.function.name.clone();
                            let _ = tx.send(ProcEvent::McpActivity(format!(
                                "[MCP] Calling {}({})",
                                name, call.function.arguments
                            )));
                            live_line_count.store(0, Ordering::Relaxed);
                            let text = match toolset
                                .call_by_name(
                                    &name,
                                    call.function.arguments.clone(),
                                    tool_live.clone(),
                                )
                                .await
                            {
                                Ok(result) => call_tool_result_text(&result),
                                Err(err) => format!("Error: {err}"),
                            };
                            // The full result still goes to the model
                            // (`messages.push` below) regardless - only the
                            // CLI Output line is shortened when the call
                            // already streamed its own progress live.
                            let result_line = if live_line_count.load(Ordering::Relaxed) > 0 {
                                "[MCP] Result: (see the streamed output above)".to_string()
                            } else {
                                format!("[MCP] Result: {}", text)
                            };
                            let _ = tx.send(ProcEvent::McpActivity(result_line));
                            messages.push(ChatMessage::tool(text));
                        }
                    }
                    Ok(format!(
                    "(stopped after {MAX_TOOL_ROUNDS} tool-calling rounds without a final reply)"
                ))
                });

            match result {
                Ok(reply) => {
                    // Tag every reply this turn didn't back with a tool call -
                    // distinct from the existing "[MCP] Calling .../[MCP]
                    // Result: ..." activity lines (note the missing `]` before
                    // the colon there) so grepping the log for one doesn't
                    // pick up the other. Ground truth for spotting a model
                    // that claims to have done something it never actually
                    // called a tool for. On its own line *after* the reply
                    // (see `MCP_NONE_TAG` in the render loop, which colors it
                    // the same Magenta as `[MCP]` activity lines) rather than
                    // appended inline, so it reads as a distinct marker, not
                    // part of the message - `split_to_entries` (called from
                    // `pump_process`) turns the `\n` into its own log line.
                    // Tagged *before* it's saved to the session, not just
                    // displayed: an untagged fabricated reply saved to
                    // history reads as established fact to whichever model
                    // sees it in a later turn (the system prompt tells it to
                    // treat a tagged prior reply as unverified instead).
                    let reply = if tool_called {
                        reply
                    } else {
                        format!("{reply}\n{MCP_NONE_TAG}")
                    };
                    session.append_user(req_message);
                    session.append_assistant(reply.clone());
                    if let Err(err) = session.save(&path) {
                        let _ = tx.send(ProcEvent::ChatReply(Err(format!(
                            "reply received but failed to save session: {err}"
                        ))));
                        continue;
                    }
                    let _ = tx.send(ProcEvent::ChatReply(Ok(format!(
                        "{OLLAMA_MARKER}\n{}: {}\n{OLLAMA_MARKER}",
                        model, reply
                    ))));
                }
                Err(err) => {
                    let _ = tx.send(ProcEvent::ChatReply(Err(format!(
                        "chat request to Ollama failed: {err}"
                    ))));
                }
            }
        }
    });

    req_tx
}

/// Appended, on its own line, after a chat reply that this turn made zero
/// tool calls for - see `spawn_chat_worker`. `spans_for_stdout_line` renders
/// a line that's exactly this tag in its own Magenta span, matching `[MCP]`
/// activity lines' color, so it reads as a distinct marker line rather than
/// part of the message.
const MCP_NONE_TAG: &str = "[MCP: NONE]";

/// Wraps a chat reply, one on its own line before and one after (see its use
/// in `spawn_chat_worker`), purely so the reply's start/end are visually
/// obvious in the CLI Output panel once other output (MCP activity, docker
/// build lines) has scrolled by in between chat turns. Display-only - not
/// saved to the session, since it carries no information the model itself
/// would benefit from seeing in later turns (unlike `MCP_NONE_TAG`). Doesn't
/// match `[MCP]`/`[DOCKER]` in `tagged_line_color`, so it already renders in
/// the same DIM grey as the reply body via that function's fallback - no
/// special-casing needed for the color, only for placement.
const OLLAMA_MARKER: &str = "[OLLAMA]";

/// Color-code CLI Output lines by their leading `[TAG]`, layered on top of
/// (not replacing) the existing per-severity colors: only `Stdout`/`Info`
/// lines are tagged this way, since `Error`/`Stderr`/`Command` already have
/// their own dedicated colors that should keep meaning "needs attention"
/// regardless of source. Falls back to `default` for untagged lines.
fn tagged_line_color(text: &str, default: Color) -> Color {
    if text.starts_with("[MCP]") {
        Color::Magenta
    } else if text.starts_with("[DOCKER]") {
        Color::Blue
    } else {
        default
    }
}

/// Split `text` on `**bold**` markdown-style markers into styled spans, so
/// AI replies (which often use `**word**` for emphasis) actually render
/// bold instead of showing the literal asterisks. Falls back to one plain
/// span for the whole line if the `**` markers aren't in balanced pairs,
/// rather than guessing - a line with a stray `**` (e.g. inside code or
/// just unbalanced markdown) is shown as-is.
fn spans_with_bold(text: &str, base: Style) -> Vec<Span<'static>> {
    let parts: Vec<&str> = text.split("**").collect();
    if parts.len() < 3 || parts.len().is_multiple_of(2) {
        return vec![Span::styled(text.to_string(), base)];
    }
    parts
        .into_iter()
        .enumerate()
        .filter(|(_, part)| !part.is_empty())
        .map(|(i, part)| {
            let style = if i % 2 == 1 {
                base.add_modifier(Modifier::BOLD)
            } else {
                base
            };
            Span::styled(part.to_string(), style)
        })
        .collect()
}

/// Builds the styled spans for a `LogEntry::Stdout` line: DIM/tagged body
/// text via `spans_with_bold`, except a line that's exactly `MCP_NONE_TAG`
/// (see `spawn_chat_worker`, which puts it on its own line after a reply)
/// renders entirely in the same Magenta as `[MCP]` activity lines.
fn spans_for_stdout_line(text: &str) -> Vec<Span<'static>> {
    match text {
        MCP_NONE_TAG => vec![Span::styled(
            text.to_string(),
            Style::default().fg(Color::Magenta),
        )],
        _ => spans_with_bold(text, Style::default().fg(tagged_line_color(text, DIM))),
    }
}

fn split_to_entries(chunk: String, is_stderr: bool) -> Vec<String> {
    let normalized = chunk.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();

    for part in normalized.split('\n') {
        if part.is_empty() {
            continue;
        }

        if is_stderr {
            if is_non_error_stderr_line(part) {
                out.push(part.to_string());
            } else {
                out.push(format!("[ERR] {}", part));
            }
        } else {
            out.push(part.to_string());
        }
    }

    out
}

fn is_non_error_stderr_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }

    // Docker BuildKit and CLI tools often print normal progress to stderr.
    if trimmed.starts_with('#') {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("sending build context")
        || lower.starts_with("step ")
        || lower.starts_with(" --->")
        || lower.starts_with("successfully built")
        || lower.starts_with("successfully tagged")
        || lower.starts_with("naming to ")
        || lower.starts_with("exporting ")
        || lower.starts_with("transferring ")
        || lower.starts_with("unpacking ")
        || lower.starts_with("load build definition")
        || lower.starts_with("load metadata")
        || lower.starts_with("load .dockerignore")
        || lower.starts_with("build context")
}

/// Blocking `docker ps` call - runs on a background thread (see
/// `AppState::refresh_docker_panel`), never on the render loop.
fn fetch_docker_panel_data() -> DockerPanelData {
    let out = Command::new("docker")
        .args(["ps", "--format", "{{.Ports}}\t{{.Image}}\t{{.Names}}"])
        .output();

    let out = match out {
        Ok(out) if out.status.success() => out,
        _ => {
            return DockerPanelData {
                available: false,
                rows: Vec::new(),
            }
        }
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let ports_raw = parts.next().unwrap_or("");
        let image = parts.next().unwrap_or("").to_string();
        let name = parts.next().unwrap_or("").to_string();

        let host_ports = extract_host_ports(ports_raw);
        let port = if host_ports.is_empty() {
            "-".to_string()
        } else {
            host_ports.join(",")
        };
        rows.push((port, image, name));
    }
    rows.sort_by(|a, b| a.2.cmp(&b.2));

    DockerPanelData {
        available: true,
        rows,
    }
}

fn extract_host_ports(line: &str) -> Vec<String> {
    line.split(',')
        .filter_map(|entry| {
            let part = entry.trim();
            if part.is_empty() {
                return None;
            }

            let mapped = part.split("->").next().unwrap_or(part).trim();
            let host_segment = mapped.rsplit(':').next().unwrap_or("").trim();
            let host_port = host_segment.split('/').next().unwrap_or("").trim();

            if !host_port.is_empty() && host_port.chars().all(|c| c.is_ascii_digit() || c == '-') {
                Some(host_port.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn menu_allowed_on_current_os(menu: MenuKind) -> bool {
    match menu {
        MenuKind::RunUbuntu22
        | MenuKind::RunUbuntu22Network
        | MenuKind::RunUbuntu22Dependencies
        | MenuKind::RunUbuntu22Github
        | MenuKind::RunUbuntu22Open
        | MenuKind::RunUbuntu22Ai
        | MenuKind::RunUbuntu22Dotfiles => !cfg!(windows),
        MenuKind::RunWin11
        | MenuKind::RunWin11Network
        | MenuKind::RunWin11Dependencies
        | MenuKind::RunWin11Github
        | MenuKind::RunWin11Open
        | MenuKind::RunWin11Ai
        | MenuKind::RunWin11Dotfiles => cfg!(windows),
        // Model selection talks to Ollama over HTTP and works on any host.
        MenuKind::Root | MenuKind::SelectModel => true,
    }
}

fn menu_items(menu: MenuKind) -> Vec<CommandItem> {
    match menu {
        MenuKind::Root => vec![
            CommandItem {
                label: "vn sys info",
                action: Action::Execute(vec!["sys", "info"]),
            },
            CommandItem {
                label: "vn bib sync (Zotero -> zotero/references.bib)",
                action: Action::Execute(vec!["bib", "sync"]),
            },
            CommandItem {
                label: "vn bib status",
                action: Action::Execute(vec!["bib", "status"]),
            },
            CommandItem {
                label: "vn run ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
            CommandItem {
                label: "vn run win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        MenuKind::RunUbuntu22 => vec![
            CommandItem {
                label: "vn run ubuntu22-ai",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Ai),
            },
            CommandItem {
                label: "vn run ubuntu22-dotfiles",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Dotfiles),
            },
            CommandItem {
                label: "vn run ubuntu22-network",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Network),
            },
            CommandItem {
                label: "vn run ubuntu22-dependencies",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Dependencies),
            },
            CommandItem {
                label: "vn run ubuntu22-github",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Github),
            },
            CommandItem {
                label: "vn run ubuntu22-open (apps + docker)",
                action: Action::OpenMenu(MenuKind::RunUbuntu22Open),
            },
            CommandItem {
                label: "< Back to Dashboard",
                action: Action::BackToRoot,
            },
        ],
        MenuKind::RunUbuntu22Network => vec![
            CommandItem {
                label: "vn net scan (rustscan open ports, local /24)",
                action: Action::Execute(vec!["net", "scan"]),
            },
            CommandItem {
                label: "vn run ubuntu22-check-internet",
                action: Action::Execute(vec!["run", "ubuntu22-check-internet"]),
            },
            CommandItem {
                label: "vn run ubuntu22-check-peripherals",
                action: Action::Execute(vec!["run", "ubuntu22-check-peripherals"]),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunUbuntu22Dependencies => vec![
            CommandItem {
                label: "vn run ubuntu22-check-dependencies",
                action: Action::Execute(vec!["run", "ubuntu22-check-dependencies"]),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunUbuntu22Github => vec![
            CommandItem {
                label: "vn run ubuntu22-download-all-repos",
                action: Action::Execute(vec!["run", "ubuntu22-download-all-repos"]),
            },
            CommandItem {
                label: "vn run ubuntu22-download-all-orgs",
                action: Action::Execute(vec!["run", "ubuntu22-download-all-orgs"]),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunUbuntu22Open => vec![
            CommandItem {
                label: "vn run ubuntu22-open-docker",
                action: Action::Execute(vec!["run", "ubuntu22-open-docker"]),
            },
            CommandItem {
                label: "vn docker check",
                action: Action::Execute(vec!["docker", "check"]),
            },
            CommandItem {
                label: "vn docker stop-all",
                action: Action::ExecuteConfirm(vec!["docker", "stop-all"]),
            },
            CommandItem {
                label: "vn docker remove-containers",
                action: Action::ExecuteConfirm(vec!["docker", "remove-containers"]),
            },
            CommandItem {
                label: "vn docker remove-images",
                action: Action::ExecuteConfirm(vec!["docker", "remove-images"]),
            },
            CommandItem {
                label: "vn app open docs",
                action: Action::Execute(vec!["app", "open", "docs"]),
            },
            CommandItem {
                label: "vn app open bentopdf",
                action: Action::Execute(vec!["app", "open", "bentopdf"]),
            },
            CommandItem {
                label: "vn app open library",
                action: Action::Execute(vec!["app", "open", "library"]),
            },
            CommandItem {
                label: "vn app open media-downloader",
                action: Action::Execute(vec!["app", "open", "media-downloader"]),
            },
            CommandItem {
                label: "vn app open doc-processor",
                action: Action::Execute(vec!["app", "open", "doc-processor"]),
            },
            CommandItem {
                label: "vn app open translator",
                action: Action::Execute(vec!["app", "open", "translator"]),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunUbuntu22Ai => vec![
            CommandItem {
                label: "vn run ubuntu22-check-ollama",
                action: Action::Execute(vec!["run", "ubuntu22-check-ollama"]),
            },
            CommandItem {
                label: "vn run ubuntu22-open-ollama",
                action: Action::Execute(vec!["run", "ubuntu22-open-ollama"]),
            },
            CommandItem {
                label: "Select Model",
                action: Action::OpenModelMenu,
            },
            CommandItem {
                label: "Download Model (type name)",
                action: Action::ArmInput(InputPurpose::DownloadModel),
            },
            CommandItem {
                label: "Chat (type message)",
                action: Action::ArmInput(InputPurpose::Chat),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunWin11 => vec![
            CommandItem {
                label: "vn run win11-ai",
                action: Action::OpenMenu(MenuKind::RunWin11Ai),
            },
            CommandItem {
                label: "vn run win11-dotfiles",
                action: Action::OpenMenu(MenuKind::RunWin11Dotfiles),
            },
            CommandItem {
                label: "vn run win11-network",
                action: Action::OpenMenu(MenuKind::RunWin11Network),
            },
            CommandItem {
                label: "vn run win11-dependencies",
                action: Action::OpenMenu(MenuKind::RunWin11Dependencies),
            },
            CommandItem {
                label: "vn run win11-github",
                action: Action::OpenMenu(MenuKind::RunWin11Github),
            },
            CommandItem {
                label: "vn run win11-open (apps + docker)",
                action: Action::OpenMenu(MenuKind::RunWin11Open),
            },
            CommandItem {
                label: "< Back to Dashboard",
                action: Action::BackToRoot,
            },
        ],
        MenuKind::RunWin11Network => vec![
            CommandItem {
                label: "vn run win11-check-peripherals",
                action: Action::Execute(vec!["run", "win11-check-peripherals"]),
            },
            CommandItem {
                label: "vn net scan (rustscan open ports, local /24)",
                action: Action::Execute(vec!["net", "scan"]),
            },
            CommandItem {
                label: "vn run win11-check-internet",
                action: Action::Execute(vec!["run", "win11-check-internet"]),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        MenuKind::RunWin11Dependencies => vec![
            CommandItem {
                label: "vn run win11-check-dependencies",
                action: Action::Execute(vec!["run", "win11-check-dependencies"]),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        MenuKind::RunWin11Github => vec![
            CommandItem {
                label: "vn run win11-download-all-repos",
                action: Action::Execute(vec!["run", "win11-download-all-repos"]),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        MenuKind::RunWin11Open => vec![
            CommandItem {
                label: "vn run win11-open-docker",
                action: Action::Execute(vec!["run", "win11-open-docker"]),
            },
            CommandItem {
                label: "vn docker check",
                action: Action::Execute(vec!["docker", "check"]),
            },
            CommandItem {
                label: "vn docker stop-all",
                action: Action::ExecuteConfirm(vec!["docker", "stop-all"]),
            },
            CommandItem {
                label: "vn docker remove-containers",
                action: Action::ExecuteConfirm(vec!["docker", "remove-containers"]),
            },
            CommandItem {
                label: "vn docker remove-images",
                action: Action::ExecuteConfirm(vec!["docker", "remove-images"]),
            },
            CommandItem {
                label: "vn app open docs",
                action: Action::Execute(vec!["app", "open", "docs"]),
            },
            CommandItem {
                label: "vn app open bentopdf",
                action: Action::Execute(vec!["app", "open", "bentopdf"]),
            },
            CommandItem {
                label: "vn app open library",
                action: Action::Execute(vec!["app", "open", "library"]),
            },
            CommandItem {
                label: "vn app open media-downloader",
                action: Action::Execute(vec!["app", "open", "media-downloader"]),
            },
            CommandItem {
                label: "vn app open doc-processor",
                action: Action::Execute(vec!["app", "open", "doc-processor"]),
            },
            CommandItem {
                label: "vn app open translator",
                action: Action::Execute(vec!["app", "open", "translator"]),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        MenuKind::RunWin11Ai => vec![
            CommandItem {
                label: "vn run win11-check-ollama",
                action: Action::Execute(vec!["run", "win11-check-ollama"]),
            },
            CommandItem {
                label: "vn run win11-open-ollama",
                action: Action::Execute(vec!["run", "win11-open-ollama"]),
            },
            CommandItem {
                label: "Select Model",
                action: Action::OpenModelMenu,
            },
            CommandItem {
                label: "Download Model (type name)",
                action: Action::ArmInput(InputPurpose::DownloadModel),
            },
            CommandItem {
                label: "Chat (type message)",
                action: Action::ArmInput(InputPurpose::Chat),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        // Unlike the win11 counterpart this is ExecuteConfirm: the Linux script
        // purges packages and masks systemd units, which is exactly what the
        // type-"yes" gate exists for.
        MenuKind::RunUbuntu22Dotfiles => vec![
            CommandItem {
                label: "vn run ubuntu22-setup-dotfiles",
                action: Action::ExecuteConfirm(vec!["run", "ubuntu22-setup-dotfiles"]),
            },
            CommandItem {
                label: "< Back to ubuntu22",
                action: Action::OpenMenu(MenuKind::RunUbuntu22),
            },
        ],
        MenuKind::RunWin11Dotfiles => vec![
            CommandItem {
                label: "vn run win11-setup-dotfiles",
                action: Action::Execute(vec!["run", "win11-setup-dotfiles"]),
            },
            CommandItem {
                label: "< Back to win11",
                action: Action::OpenMenu(MenuKind::RunWin11),
            },
        ],
        // Built dynamically from installed models in `set_menu`; never queried here.
        MenuKind::SelectModel => Vec::new(),
    }
}

/// A bordered panel. When `focused` it gets a bright accent border and a
/// highlighted (light-blue) title label; otherwise a dim border + muted title.
/// This shows which panel (Dashboard / Input) is active without flooding the
/// terminal's black background.
fn panel_block(title: &str, focused: bool) -> Block<'static> {
    let (border, title_style) = if focused {
        (
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (Style::default().fg(DIM), Style::default().fg(MUTED))
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(Span::styled(format!(" {} ", title), title_style))
}

/// A plain (non-focusable) panel: dim border, muted title — for the static
/// panels (CLI header, Docker, CLI Output).
fn plain_block(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default().fg(MUTED),
        ))
}

pub fn run(repo_root: Option<std::path::PathBuf>, loaded: LoadedConfig) -> Result<()> {
    if let Some(repo_root) = &repo_root {
        env::set_var("VNCLI_REPO_ROOT", repo_root);
        env::set_current_dir(repo_root)?;
    }

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, repo_root, loaded);

    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    repo_root: Option<std::path::PathBuf>,
    loaded: LoadedConfig,
) -> Result<()> {
    let mut app = AppState::new(repo_root, loaded);
    const DOCKER_REFRESH_INTERVAL: Duration = Duration::from_secs(4);

    loop {
        app.pump_process();
        app.pump_mcp_approvals();
        app.pump_docker_panel();

        if app.last_docker_refresh.elapsed() >= DOCKER_REFRESH_INTERVAL {
            app.refresh_docker_panel();
            app.last_docker_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc => match app.focus {
                        Focus::Input => {
                            app.input.clear();
                            app.input_purpose = InputPurpose::None;
                            app.focus = Focus::Dashboard;
                        }
                        Focus::Running => {
                            app.focus = Focus::Dashboard;
                        }
                        Focus::Dashboard => {
                            if let Some(parent) = app.parent_menu() {
                                app.set_menu(parent);
                            } else {
                                app.shutdown();
                                break;
                            }
                        }
                    },
                    KeyCode::Char('q') if !matches!(app.focus, Focus::Input) => {
                        app.shutdown();
                        break;
                    }
                    KeyCode::Tab => {
                        app.focus = match app.focus {
                            Focus::Dashboard => Focus::Running,
                            Focus::Running => Focus::Input,
                            Focus::Input => Focus::Dashboard,
                        }
                    }
                    KeyCode::Down => match app.focus {
                        Focus::Dashboard => app.next(),
                        Focus::Running => app.running_next(),
                        Focus::Input => {}
                    },
                    KeyCode::Up => match app.focus {
                        Focus::Dashboard => app.previous(),
                        Focus::Running => app.running_previous(),
                        Focus::Input => {}
                    },
                    KeyCode::Char('j') if !matches!(app.focus, Focus::Input) => {
                        if matches!(app.focus, Focus::Running) {
                            app.running_next();
                        } else {
                            app.next();
                        }
                    }
                    KeyCode::Char('k') if !matches!(app.focus, Focus::Input) => {
                        if matches!(app.focus, Focus::Running) {
                            app.running_previous();
                        } else {
                            app.previous();
                        }
                    }
                    KeyCode::Char('x') if !matches!(app.focus, Focus::Input) => {
                        app.kill_all_running();
                    }
                    KeyCode::Char('e') if !matches!(app.focus, Focus::Input) => {
                        app.jump_to_previous_error();
                    }
                    KeyCode::Char(',') if !matches!(app.focus, Focus::Input) => {
                        app.output_page_up();
                    }
                    KeyCode::Char('.') if !matches!(app.focus, Focus::Input) => {
                        app.output_page_down();
                    }
                    KeyCode::Char('r') | KeyCode::Char('R')
                        if !matches!(app.focus, Focus::Input) =>
                    {
                        app.refresh_ui();
                        terminal.clear()?;
                    }
                    KeyCode::Enter => match app.focus {
                        Focus::Dashboard => app.activate_selected(),
                        Focus::Running => app.kill_selected_running(),
                        Focus::Input => app.send_input_line(),
                    },
                    KeyCode::Backspace => {
                        if matches!(app.focus, Focus::Input) {
                            app.input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if matches!(app.focus, Focus::Input) {
                            app.input.push(c);
                        }
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => app.output_line_up(),
                    MouseEventKind::ScrollDown => app.output_line_down(),
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.click_dashboard_row(mouse.column, mouse.row);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    terminal.clear()?;
                }
                Event::FocusGained | Event::FocusLost => {}
                _ => {}
            }
        }

        terminal.draw(|frame| {
            let areas = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(4),
                ])
                .split(frame.area());

            let active_model = app.selected_model.clone().unwrap_or_else(|| "none".to_string());
            let today = Local::now().format("%Y-%m-%d");
            let header = Paragraph::new(format!(
                "vncli vn    |    AI model: {}    |    Date: {}    |    Running: {}",
                active_model,
                today,
                app.running.len()
            ))
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .block(plain_block("CLI".to_string()));

            let middle = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(areas[1]);

            let left_panels = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                ])
                .split(middle[0]);

            app.last_dashboard_area = left_panels[0];

            // The model menu lists installed models (or a placeholder); every
            // other menu lists its CommandItem labels.
            let (labels, dashboard_title): (Vec<String>, &str) =
                if app.menu == MenuKind::SelectModel {
                    if app.model_items.is_empty() {
                        (
                            vec!["(no tool-capable models - try 'llama3.2' via Download Model)".to_string()],
                            "Select Model",
                        )
                    } else {
                        (app.model_items.clone(), "Select Model")
                    }
                } else {
                    (
                        app.commands.iter().map(|cmd| cmd.label.to_string()).collect(),
                        "Dashboard",
                    )
                };

            let button_items: Vec<ListItem> = labels
                .iter()
                .enumerate()
                .map(|(idx, label)| {
                    if idx == app.selected {
                        ListItem::new(Line::from(vec![Span::styled(
                            format!("[ {} ]", label),
                            Style::default()
                                .fg(Color::Black)
                                .bg(ACCENT)
                                .add_modifier(Modifier::BOLD),
                        )]))
                    } else {
                        ListItem::new(Line::from(vec![Span::styled(
                            format!("[ {} ]", label),
                            Style::default().fg(Color::White),
                        )]))
                    }
                })
                .collect();

            let mut list_state = ListState::default();
            list_state.select(Some(app.selected));

            let dashboard = List::new(button_items)
                .block(panel_block(dashboard_title, matches!(app.focus, Focus::Dashboard)));

            let docker_status = if app.docker_panel.available { "ON" } else { "OFF" };
            let docker_header = Row::new(vec!["Port", "Image", "Container"])
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
            let docker_rows: Vec<Row> = if app.docker_panel.available && app.docker_panel.rows.is_empty()
            {
                vec![Row::new(vec!["-", "(no running containers)", "-"])]
            } else if !app.docker_panel.available {
                vec![Row::new(vec!["-", "(docker not running)", "-"])]
            } else {
                app.docker_panel
                    .rows
                    .iter()
                    .map(|(p, i, c)| Row::new(vec![p.clone(), i.clone(), c.clone()]))
                    .collect()
            };
            let docker = Table::new(
                docker_rows,
                [
                    Constraint::Percentage(13),
                    Constraint::Percentage(59),
                    Constraint::Percentage(28),
                ],
            )
            .header(docker_header)
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(DIM))
                    .title(Span::styled(
                        format!(" Docker: {} ", docker_status),
                        Style::default()
                            .fg(if app.docker_panel.available { ACCENT } else { DIM })
                            .add_modifier(Modifier::BOLD),
                    )),
            );

            let running_focused = matches!(app.focus, Focus::Running);
            let running_items: Vec<ListItem> = if app.running.is_empty() {
                vec![ListItem::new(Line::from(Span::styled(
                    "(no running processes)",
                    Style::default().fg(MUTED),
                )))]
            } else {
                app.running
                    .iter()
                    .enumerate()
                    .map(|(idx, proc)| {
                        let text = format!(
                            "{}  ({}s)",
                            proc.label,
                            proc.started_at.elapsed().as_secs()
                        );
                        if running_focused && idx == app.running_selected {
                            ListItem::new(Line::from(Span::styled(
                                text,
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(ACCENT)
                                    .add_modifier(Modifier::BOLD),
                            )))
                        } else {
                            ListItem::new(Line::from(Span::styled(
                                text,
                                Style::default().fg(Color::White),
                            )))
                        }
                    })
                    .collect()
            };
            let running_panel =
                List::new(running_items).block(panel_block("Running (Enter: kill)", running_focused));

            let mcp_status_line = format!(
                "Apps toolset: {} tool(s)  |  HTTP: 127.0.0.1:{}",
                crate::mcp::apps_toolset::TOOL_COUNT,
                app.mcp_port
            );
            let mcp_panel = Paragraph::new(mcp_status_line)
                .style(Style::default().fg(MUTED))
                .block(plain_block("MCP Server".to_string()));

            let log_lines: Vec<Line> = app
                .logs
                .iter()
                .map(|entry| {
                    if let LogEntry::Command(command) = entry {
                        Line::from(Span::styled(
                            format!("> {}", command),
                            Style::default()
                                .fg(ACCENT)
                                .add_modifier(Modifier::BOLD),
                        ))
                    } else if let LogEntry::Error(text) = entry {
                        Line::from(Span::styled(text.clone(), Style::default().fg(Color::LightRed)))
                    } else if let LogEntry::Stderr(text) = entry {
                        Line::from(Span::styled(text.clone(), Style::default().fg(Color::Yellow)))
                    } else if let LogEntry::Stdout(text) = entry {
                        // Body text (docker build/log passthrough, AI chat
                        // replies) uses the same dark-grey tone as dim
                        // chrome (DIM) rather than stark white - softer for
                        // long-form reading. [INFO] keeps its own MUTED tone
                        // unchanged below. `**bold**` markers (common in AI
                        // replies) render bold instead of literal asterisks.
                        Line::from(spans_for_stdout_line(text))
                    } else if let LogEntry::Mcp(text) = entry {
                        // Every line an MCP tool call produced defaults to
                        // Magenta - not just the one line that happens to
                        // literally start with "[MCP]" - unless it carries
                        // its own more specific tag (a live-streamed docker
                        // build's "[DOCKER]" lines stay Blue, matching those
                        // same lines outside of MCP).
                        Line::from(spans_with_bold(
                            text,
                            Style::default().fg(tagged_line_color(text, Color::Magenta)),
                        ))
                    } else {
                        match entry {
                            LogEntry::Info(text) => Line::from(spans_with_bold(
                                text,
                                Style::default().fg(tagged_line_color(text, MUTED)),
                            )),
                            _ => Line::from(Span::raw(String::new())),
                        }
                    }
                })
                .collect();

            app.output_view_lines = middle[1].height.saturating_sub(2) as usize;

            let output = Paragraph::new(log_lines)
                .block(plain_block("CLI Output".to_string()))
                .wrap(Wrap { trim: false });
            // `output_scroll` is a wrapped-*row* offset once `Wrap` is on,
            // not a log-*entry* offset - `line_count` (ratatui's own wrap
            // calculation, not an approximation) is what `max_output_scroll`
            // needs to follow/page against the true rendered bottom instead
            // of stopping short whenever a long line (a docker build command,
            // say) wraps into more than one row. Must run before `.scroll()`
            // below, which consumes `output` to attach the now-correct offset.
            app.output_total_rows = output.line_count(middle[1].width).max(1);
            app.clamp_output_scroll();
            let output = output.scroll((app.output_scroll, 0));

            let keys_text = match app.focus {
                Focus::Dashboard => {
                    "Tab: running  Up/Down/click: select  Enter: run  R: refresh ui  ,/.: output page  x: stop all  e: last error  q: quit  Esc: back"
                }
                Focus::Running => {
                    "Tab: input  Up/Down: select  Enter: kill selected  x: stop all  Esc: dashboard"
                }
                Focus::Input => {
                    "Type to enter text  Tab: dashboard  Enter: send input  Backspace: delete  Esc: cancel"
                }
            };

            let input_text = if !app.input.is_empty() {
                app.input.clone()
            } else {
                match app.input_purpose.clone() {
                    InputPurpose::DownloadModel => {
                        "Type a model name to download, then Enter...".to_string()
                    }
                    InputPurpose::Chat => format!(
                        "Message {} , then Enter...",
                        app.selected_model.clone().unwrap_or_else(|| "none".to_string())
                    ),
                    InputPurpose::ConfirmDestructive(_, label) => {
                        format!("Type 'yes' to confirm '{}', or Tab to cancel...", label)
                    }
                    InputPurpose::ApproveMcp(description) => {
                        format!(
                            "MCP approval: {} - type 'yes' to approve, anything else to deny...",
                            description
                        )
                    }
                    InputPurpose::None => "Type input for running command...".to_string(),
                }
            };

            let input_focused = matches!(app.focus, Focus::Input);
            let input_style = if input_focused {
                Style::default().fg(Color::Black).bg(ACCENT)
            } else {
                Style::default().fg(DIM)
            };

            let footer = Paragraph::new(vec![
                Line::from(Span::styled(input_text, input_style)),
                Line::from(Span::styled(keys_text, Style::default().fg(MUTED))),
            ])
            .block(panel_block("Input", input_focused));

            frame.render_widget(header, areas[0]);
            frame.render_stateful_widget(dashboard, left_panels[0], &mut list_state);
            frame.render_widget(docker, left_panels[1]);
            frame.render_widget(running_panel, left_panels[2]);
            frame.render_widget(mcp_panel, left_panels[3]);
            frame.render_widget(output, middle[1]);
            frame.render_widget(footer, areas[2]);
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    /// Guards against `build_ollama_tools`'s `filter_map(...).ok()?` silently
    /// dropping a tool whose JSON schema fails to parse into
    /// `schemars::Schema` - every tool the toolset registers should make it
    /// into the Ollama-facing tool list.
    #[test]
    fn build_ollama_tools_includes_every_registered_tool() {
        let loaded = LoadedConfig {
            path: std::path::PathBuf::from("test-config.toml"),
            config: AppConfig::default(),
        };
        let (approval, _rx) = ApprovalGate::new();
        let toolset = AppsToolset::new(loaded, approval, true);

        let tools = build_ollama_tools(&toolset);

        assert_eq!(tools.len(), crate::mcp::apps_toolset::TOOL_COUNT);
        let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"list_apps"));
        assert!(names.contains(&"open_app"));
        assert!(names.contains(&"stop_app"));
    }

    #[test]
    fn spans_with_bold_splits_balanced_markers() {
        let spans = spans_with_bold("hello **world** foo", Style::default());
        let rendered: Vec<(String, bool)> = spans
            .iter()
            .map(|s| {
                (
                    s.content.to_string(),
                    s.style.add_modifier.contains(Modifier::BOLD),
                )
            })
            .collect();
        assert_eq!(
            rendered,
            vec![
                ("hello ".to_string(), false),
                ("world".to_string(), true),
                (" foo".to_string(), false),
            ]
        );
    }

    #[test]
    fn spans_with_bold_leaves_unbalanced_markers_plain() {
        let spans = spans_with_bold("hello **world", Style::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.to_string(), "hello **world");
        assert!(!spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn spans_with_bold_leaves_plain_text_untouched() {
        let spans = spans_with_bold("no markers here", Style::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.to_string(), "no markers here");
    }

    #[test]
    fn spans_for_stdout_line_renders_mcp_none_tag_line_entirely_magenta() {
        // `spawn_chat_worker` puts the tag on its own line (see
        // `MCP_NONE_TAG`'s doc), so by the time it reaches this function it's
        // always the whole line, not a suffix of the reply's last line.
        let spans = spans_for_stdout_line(MCP_NONE_TAG);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.to_string(), "[MCP: NONE]");
        assert_eq!(spans[0].style.fg, Some(Color::Magenta));
    }

    #[test]
    fn spans_for_stdout_line_leaves_untagged_text_as_one_span() {
        let spans = spans_for_stdout_line("just a normal reply");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.to_string(), "just a normal reply");
        assert_eq!(spans[0].style.fg, Some(DIM));
    }

    #[test]
    fn spans_for_stdout_line_renders_ollama_marker_in_same_dim_as_reply_body() {
        // OLLAMA_MARKER deliberately doesn't match `[MCP]`/`[DOCKER]` in
        // tagged_line_color, so it should render identically to a plain
        // reply line - same color, no special-casing needed for that part.
        let spans = spans_for_stdout_line(OLLAMA_MARKER);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.to_string(), "[OLLAMA]");
        assert_eq!(spans[0].style.fg, Some(DIM));
    }
}
