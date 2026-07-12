use crate::config::LoadedConfig;
use crate::RunArgs;
use anyhow::{anyhow, bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn run(args: RunArgs, loaded: &LoadedConfig) -> Result<()> {
    run_named_script(&args.name, loaded)
}

pub fn run_named_script(name: &str, loaded: &LoadedConfig) -> Result<()> {
    // Container tasks were moved from per-OS scripts into Rust (commands::apps);
    // legacy names keep working by routing there.
    if let Some(result) = try_native(name, loaded) {
        return result;
    }

    let repo_root = detect_repo_root(loaded)?;
    let target = map_script(name)?;
    let path = repo_root.join(target.relative_path);

    if !path.exists() {
        bail!("script not found: {}", path.display());
    }

    let status =
        run_script(&path).with_context(|| format!("failed running script: {}", path.display()))?;
    if !status.success() {
        return Err(anyhow!("script exited with status: {}", status));
    }

    Ok(())
}

/// Legacy script names now handled natively by `vn app` / `vn docker`.
fn try_native(name: &str, loaded: &LoadedConfig) -> Option<Result<()>> {
    use crate::commands::apps;
    let key = name.to_ascii_lowercase();
    let stripped = key
        .strip_prefix("win11-")
        .or_else(|| key.strip_prefix("ubuntu22-"))
        .unwrap_or(&key);
    let result = match stripped {
        "open-docs" => apps::open("docs", loaded, false),
        "silverbullet" | "run-silverbullet" | "open-silverbullet" => {
            apps::open("silverbullet", loaded, false)
        }
        "open-bentopdf" => apps::open("bentopdf", loaded, false),
        "stop-bentopdf" => apps::stop("bentopdf", loaded),
        "open-library" => apps::open("library", loaded, false),
        "stop-library" => apps::stop("library", loaded),
        "open-media-downloader" => apps::open("media-downloader", loaded, false),
        "stop-media-downloader" => apps::stop("media-downloader", loaded),
        "open-doc-processor" => apps::open("doc-processor", loaded, false),
        "check-docker" => apps::docker_check(),
        "stop-all-containers" => apps::docker_stop_all(),
        "remove-containers" | "remove-all-containers" => apps::docker_remove_containers(),
        "remove-images" | "remove-all-images" => apps::docker_remove_images(),
        _ => return None,
    };
    Some(result)
}

struct ScriptTarget {
    relative_path: &'static str,
}

fn map_script(name: &str) -> Result<ScriptTarget> {
    let key = name.to_ascii_lowercase();
    let is_linux = cfg!(target_os = "linux");
    let target = match key.as_str() {
        "ubuntu22" => ScriptTarget {
            relative_path: "scripts/ubuntu22/main.sh",
        },
        "ubuntu22-check-internet" => ScriptTarget {
            relative_path: "scripts/ubuntu22/check_internet.sh",
        },
        "ubuntu22-check-dependencies" => ScriptTarget {
            relative_path: "scripts/ubuntu22/check_dependencies.sh",
        },
        "ubuntu22-download-all-repos" => ScriptTarget {
            relative_path: "scripts/ubuntu22/download_all_repos.sh",
        },
        "ubuntu22-download-all-orgs" => ScriptTarget {
            relative_path: "scripts/ubuntu22/download_all_orgs.sh",
        },
        "ubuntu22-open-docker" => ScriptTarget {
            relative_path: "scripts/ubuntu22/open_docker.sh",
        },
        "ubuntu22-run-cli-container" => ScriptTarget {
            relative_path: "scripts/ubuntu22/run_cli_container.sh",
        },
        "ubuntu22-check-ollama" => ScriptTarget {
            relative_path: "scripts/ubuntu22/check_ollama.sh",
        },
        "ubuntu22-open-ollama" => ScriptTarget {
            relative_path: "scripts/ubuntu22/open_ollama.sh",
        },
        "check-internet" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/check_internet.sh"
            } else {
                "scripts/win11/check_internet.bat"
            },
        },
        "check-dependencies" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/check_dependencies.sh"
            } else {
                "scripts/win11/check_dependencies.bat"
            },
        },
        "open-docker" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/open_docker.sh"
            } else {
                "scripts/win11/open_docker.bat"
            },
        },
        "download-all-repos" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/download_all_repos.sh"
            } else {
                "scripts/win11/download_all_repos.bat"
            },
        },
        "download-all-orgs" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/download_all_orgs.sh"
            } else {
                "scripts/win11/download_all_orgs.bat"
            },
        },
        "run-cli-container" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/run_cli_container.sh"
            } else {
                "scripts/win11/run_cli_container.bat"
            },
        },
        "check-ollama" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/check_ollama.sh"
            } else {
                "scripts/win11/check_ollama.bat"
            },
        },
        "open-ollama" => ScriptTarget {
            relative_path: if is_linux {
                "scripts/ubuntu22/open_ollama.sh"
            } else {
                "scripts/win11/open_ollama.bat"
            },
        },
        "win11" => ScriptTarget {
            relative_path: "scripts/win11/main.bat",
        },
        "win11-check-peripherals" => ScriptTarget {
            relative_path: "scripts/win11/check_peripherals.bat",
        },
        "ubuntu22-check-peripherals" => ScriptTarget {
            relative_path: "scripts/ubuntu22/check_peripherals.sh",
        },
        "win11-check-internet" => ScriptTarget {
            relative_path: "scripts/win11/check_internet.bat",
        },
        "win11-check-dependencies" => ScriptTarget {
            relative_path: "scripts/win11/check_dependencies.bat",
        },
        "win11-download-all-repos" => ScriptTarget {
            relative_path: "scripts/win11/download_all_repos.bat",
        },
        "win11-open-docker" => ScriptTarget {
            relative_path: "scripts/win11/open_docker.bat",
        },
        "win11-download-all-orgs" => ScriptTarget {
            relative_path: "scripts/win11/download_all_orgs.bat",
        },
        "win11-run-cli-container" => ScriptTarget {
            relative_path: "scripts/win11/run_cli_container.bat",
        },
        "win11-check-ollama" => ScriptTarget {
            relative_path: "scripts/win11/check_ollama.bat",
        },
        "win11-open-ollama" => ScriptTarget {
            relative_path: "scripts/win11/open_ollama.bat",
        },
        "win11-setup-dotfiles" => ScriptTarget {
            relative_path: "dotfiles/win11/setup_dotfiles.bat",
        },
        "tools-alpine" => ScriptTarget {
            relative_path: "scripts/tools-cli/alpine/main.sh",
        },
        _ => bail!(
            "unknown script name '{}'. Supported linux and win11 script names plus cross-platform aliases (e.g. check-internet, check-dependencies, open-docker, open-docs, open-doc-processor, check-ollama, open-ollama, download-all-repos, download-all-orgs, run-cli-container, open-silverbullet)",
            name
        ),
    };

    Ok(target)
}

fn run_script(script: &Path) -> Result<std::process::ExitStatus> {
    let ext = script
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut cmd = if ext == "bat" {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(script);
            c
        } else {
            bail!("cannot execute .bat script on non-Windows host")
        }
    } else if cfg!(windows) {
        let mut c = Command::new("wsl");
        c.arg("bash").arg(windows_path_to_wsl(script)?);
        c
    } else {
        let mut c = Command::new("bash");
        c.arg(script);
        c
    };

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    cmd.status().context("failed to spawn process")
}

fn windows_path_to_wsl(path: &Path) -> Result<String> {
    let raw = path
        .canonicalize()
        .with_context(|| format!("failed to resolve script path: {}", path.display()))?;

    let raw_str = raw
        .to_str()
        .ok_or_else(|| anyhow!("script path is not valid unicode"))?;

    // Canonical Windows paths may use the extended-length prefix (e.g. \\\\?\\C:\\...)
    // which must be removed before drive-letter parsing.
    let normalized = if let Some(stripped) = raw_str.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = raw_str.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw_str.to_string()
    };

    if normalized.starts_with(r"\\") {
        bail!(
            "UNC paths are not supported for WSL script execution: {}",
            normalized
        );
    }

    let mut chars = normalized.chars();
    let drive = chars
        .next()
        .ok_or_else(|| anyhow!("invalid script path"))?
        .to_ascii_lowercase();
    let colon = chars.next().ok_or_else(|| anyhow!("invalid script path"))?;

    if !drive.is_ascii_alphabetic() || colon != ':' {
        bail!("unsupported Windows path format: {}", normalized);
    }

    let rest = chars.as_str().replace('\\', "/");
    Ok(format!("/mnt/{}/{}", drive, rest.trim_start_matches('/')))
}

pub(crate) fn detect_repo_root(loaded: &LoadedConfig) -> Result<PathBuf> {
    if let Ok(path) = env::var("VNCLI_REPO_ROOT") {
        let p = PathBuf::from(&path);
        if is_valid_repo_root(&p) {
            return Ok(p);
        } else {
            eprintln!(
                "WARNING: VNCLI_REPO_ROOT={} does not look like a valid vncli repository (missing scripts/ or docker/ directory)",
                path
            );
        }
    }

    if let Some(from_config) = loaded.path.parent().and_then(Path::parent) {
        if is_valid_repo_root(from_config) {
            return Ok(from_config.to_path_buf());
        }
    }

    let cwd = env::current_dir().context("failed to read current directory")?;
    for ancestor in cwd.ancestors() {
        if is_valid_repo_root(ancestor) {
            return Ok(ancestor.to_path_buf());
        }
    }

    bail!("could not locate repository root. Run inside the vncli repo or set VNCLI_REPO_ROOT")
}

/// Validate that a path looks like a vncli repo/distribution root. Deliberately
/// does not require `.git`: `distribute_cli.bat` ships a plain folder (no git
/// checkout) that still needs `vn app open <name>` to find `docker/` and
/// `scripts/` for its build contexts and helper scripts.
fn is_valid_repo_root(path: &Path) -> bool {
    let has_scripts = path.join("scripts").exists();
    let has_docker = path.join("docker").exists();
    has_scripts && has_docker
}
