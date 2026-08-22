use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub config: AppConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ollama: OllamaConfig,
    pub sessions: SessionsConfig,
    pub prompts: PromptConfig,
    /// `#[serde(default)]` matters: config.toml files written before this
    /// section existed have no `[zotero]` table, and `load_or_init` never
    /// rewrites an existing file. Without it every older install would fail
    /// to parse its own config.
    #[serde(default)]
    pub zotero: ZoteroConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub host: String,
    pub model: String,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsConfig {
    pub dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub system: Option<String>,
}

/// Both default to `None` rather than a concrete path: `vn bib` falls back to
/// `~/Zotero` (Zotero's default on every OS) and to `<repo>/zotero/references.bib`,
/// which keeps a config written on one machine valid on the other.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZoteroConfig {
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub bib_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let sessions_dir = default_sessions_dir();
        Self {
            ollama: OllamaConfig {
                host: "http://127.0.0.1:11434".to_string(),
                model: "llama3.2".to_string(),
                stream: true,
            },
            sessions: SessionsConfig {
                dir: sessions_dir.to_string_lossy().to_string(),
            },
            prompts: PromptConfig {
                system: Some(default_system_prompt()),
            },
            zotero: ZoteroConfig::default(),
        }
    }
}

pub fn load_or_init(override_path: Option<PathBuf>) -> Result<LoadedConfig> {
    let path = override_path.unwrap_or_else(default_config_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
        // Set directory permissions to 0o700 (user only) on Unix
        set_dir_permissions(parent)?;
    }

    let config = if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        toml::from_str::<AppConfig>(&content)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?
    } else {
        let cfg = AppConfig::default();
        let toml_content =
            toml::to_string_pretty(&cfg).context("failed to serialize default config")?;
        fs::write(&path, toml_content)
            .with_context(|| format!("failed to write default config: {}", path.display()))?;
        // Set file permissions to 0o600 (user only) on Unix
        set_file_permissions(&path)?;
        cfg
    };

    let sessions_dir = expand_tilde(&config.sessions.dir);
    fs::create_dir_all(&sessions_dir)
        .with_context(|| format!("failed to create sessions dir: {}", sessions_dir.display()))?;
    // Set directory permissions to 0o700 (user only) on Unix
    set_dir_permissions(&sessions_dir)?;

    Ok(LoadedConfig { path, config })
}

pub fn default_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("vn").join("config.toml")
}

fn default_sessions_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("vn").join("sessions")
}

/// Only applied to a *freshly generated* `config.toml` (`load_or_init` never
/// overwrites an existing one) - editing this doesn't change what an
/// existing install already has saved. Written to steer small/local models
/// away from the two failure modes actually observed in the TUI chat: (1)
/// answering a state question (what's running, what containers exist) from
/// memory or a plausible guess instead of calling the tool that would give a
/// real answer, and (2) treating a `[MCP: NONE]`-tagged prior reply in this
/// same conversation as established fact just because it's in the history.
fn default_system_prompt() -> String {
    "You are a local offline systems assistant for vncli. For any question about current \
     state - which apps or containers exist, whether one is running, what processes are on \
     this host, disk usage, log contents - call the matching tool and answer from its result. \
     Never guess, assume, or invent identifiers, ports, or statuses; if you have not called a \
     tool this turn, say so instead of describing what you'd expect to be true. State can \
     change between turns, so re-call the tool even if you or an earlier turn already answered \
     a similar question - do not just repeat a previous answer. If an earlier assistant message \
     in this conversation ends with the line \"[MCP: NONE]\", nothing in it was backed by a \
     tool call - treat its contents as unverified, not as fact."
        .to_string()
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(input));
    }

    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    Path::new(input).to_path_buf()
}

/// Set file permissions to 0o600 (user read/write only) on Unix.
/// On Windows, this is a no-op (NTFS ACLs are inherited from parent).
#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<()> {
    let perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, perms).with_context(|| {
        format!(
            "failed to set file permissions (0o600) for: {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<()> {
    // On Windows, permissions are inherited from the config directory created by `dirs::config_dir()`,
    // which respects user profile ACLs. No additional action needed.
    Ok(())
}

/// Set directory permissions to 0o700 (user only) on Unix.
/// On Windows, this is a no-op (NTFS ACLs are inherited from parent).
#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> Result<()> {
    let perms = fs::Permissions::from_mode(0o700);
    fs::set_permissions(path, perms).with_context(|| {
        format!(
            "failed to set directory permissions (0o700) for: {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path) -> Result<()> {
    // On Windows, permissions are inherited from parent directories created by `dirs::*_dir()` crates.
    // No additional action needed.
    Ok(())
}
