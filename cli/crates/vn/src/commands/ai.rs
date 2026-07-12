use crate::config::{expand_tilde, LoadedConfig};
use crate::ollama::session::{session_path, SessionFile};
use crate::{AiArgs, AiCommand};
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use ollama_rs::generation::chat::{request::ChatMessageRequest, ChatMessage};
use ollama_rs::Ollama;
use std::io::Write;

pub async fn run(args: AiArgs, loaded: &LoadedConfig) -> Result<()> {
    let host = args
        .host
        .clone()
        .unwrap_or_else(|| loaded.config.ollama.host.clone());
    let ollama = build_client(&host);

    match args.command {
        AiCommand::Status => status(&ollama, &host).await,
        AiCommand::Models { tools_only } => models(&ollama, tools_only).await,
        AiCommand::Pull { name } => pull(&ollama, &name).await,
        AiCommand::Chat {
            message,
            model,
            session,
            system,
        } => {
            let model = model.unwrap_or_else(|| loaded.config.ollama.model.clone());
            let session = session.unwrap_or_else(|| "tui".to_string());
            let system = system.or_else(|| loaded.config.prompts.system.clone());
            chat(
                &ollama,
                loaded,
                &model,
                &session,
                system.as_deref(),
                &message,
            )
            .await
        }
    }
}

/// Build an Ollama client from a configured host URL such as
/// `http://127.0.0.1:11434`. `Ollama::new` is deprecated upstream but is the
/// stable way to point at a specific host/port across ollama-rs versions.
#[allow(deprecated)]
pub(crate) fn build_client(host: &str) -> Ollama {
    let (host, port) = split_host_port(host, 11434);
    Ollama::new(host, port)
}

/// Split a URL like `http://127.0.0.1:11434` into (`http://127.0.0.1`, 11434).
/// Falls back to the default port when no numeric port is present.
fn split_host_port(host: &str, default_port: u16) -> (String, u16) {
    if let Some((head, tail)) = host.rsplit_once(':') {
        if let Ok(port) = tail.parse::<u16>() {
            return (head.to_string(), port);
        }
    }
    (host.to_string(), default_port)
}

async fn status(ollama: &Ollama, host: &str) -> Result<()> {
    match ollama.list_local_models().await {
        Ok(models) => {
            println!(
                "[OK] Ollama is reachable at {} ({} model(s) installed).",
                host,
                models.len()
            );
            Ok(())
        }
        Err(err) => {
            println!("[ERROR] Could not reach Ollama at {}: {}", host, err);
            println!("[INFO] Start it with the 'Open Ollama' command, then try again.");
            bail!("ollama is not reachable");
        }
    }
}

/// Print installed model names, one per line, to stdout. Kept free of other
/// output so the TUI can parse it directly to build the model menu. With
/// `tools_only`, skips models that don't report the `tools` capability (the
/// TUI chat always attaches tools, so a model without it can't chat at all -
/// see `model_supports_tools`).
async fn models(ollama: &Ollama, tools_only: bool) -> Result<()> {
    let models = ollama
        .list_local_models()
        .await
        .context("failed to list models from Ollama")?;

    if models.is_empty() {
        eprintln!(
            "[INFO] No models installed. Use 'vn ai pull <name>' or the Download Model button."
        );
        return Ok(());
    }

    for model in models {
        if tools_only && !model_supports_tools(ollama, &model.name).await {
            continue;
        }
        println!("{}", model.name);
    }
    Ok(())
}

/// Whether `name` reports the `tools` capability via `ollama show`. Treated
/// as unsupported (rather than erroring) if the lookup itself fails, so one
/// broken model doesn't break listing the rest.
async fn model_supports_tools(ollama: &Ollama, name: &str) -> bool {
    ollama
        .show_model_info(name.to_string())
        .await
        .map(|info| info.capabilities.iter().any(|c| c == "tools"))
        .unwrap_or(false)
}

async fn pull(ollama: &Ollama, name: &str) -> Result<()> {
    println!("[INFO] Downloading model '{}'...", name);
    let _ = std::io::stdout().flush();

    let mut stream = ollama
        .pull_model_stream(name.to_string(), false)
        .await
        .with_context(|| format!("failed to start download of model '{}'", name))?;

    // Stream progress as discrete lines so the TUI/log show live status without
    // flooding: emit only when the phase or the whole-percent changes.
    let mut last_message = String::new();
    let mut last_percent: i64 = -1;
    while let Some(item) = stream.next().await {
        let status =
            item.map_err(|err| anyhow!("error downloading model '{}': {:?}", name, err))?;

        match (status.completed, status.total) {
            (Some(done), Some(total)) if total > 0 => {
                let percent = (done as f64 / total as f64 * 100.0) as i64;
                if status.message != last_message || percent != last_percent {
                    println!("[INFO] {} - {}%", status.message, percent);
                    let _ = std::io::stdout().flush();
                    last_message = status.message.clone();
                    last_percent = percent;
                }
            }
            _ => {
                if status.message != last_message {
                    println!("[INFO] {}", status.message);
                    let _ = std::io::stdout().flush();
                    last_message = status.message.clone();
                    last_percent = -1;
                }
            }
        }
    }

    println!("[OK] Model '{}' downloaded and ready.", name);
    Ok(())
}

async fn chat(
    ollama: &Ollama,
    loaded: &LoadedConfig,
    model: &str,
    session_name: &str,
    system: Option<&str>,
    message: &str,
) -> Result<()> {
    let sessions_dir = expand_tilde(&loaded.config.sessions.dir);
    let path = session_path(&sessions_dir, session_name);
    let mut session = SessionFile::load_or_new(&path, session_name)?;

    // Replay prior turns so the model keeps context across invocations.
    let mut messages: Vec<ChatMessage> = Vec::new();
    if let Some(system_prompt) = system {
        messages.push(ChatMessage::system(system_prompt.to_string()));
    }
    for msg in &session.messages {
        match msg.role.as_str() {
            "user" => messages.push(ChatMessage::user(msg.content.clone())),
            "assistant" => messages.push(ChatMessage::assistant(msg.content.clone())),
            "system" => messages.push(ChatMessage::system(msg.content.clone())),
            _ => {}
        }
    }
    messages.push(ChatMessage::user(message.to_string()));

    let request = ChatMessageRequest::new(model.to_string(), messages);
    let response = ollama
        .send_chat_messages(request)
        .await
        .context("chat request to Ollama failed")?;
    let reply = response.message.content;

    println!("{}: {}", model, reply);

    session.append_user(message.to_string());
    session.append_assistant(reply);
    session.save(&path)?;

    Ok(())
}
