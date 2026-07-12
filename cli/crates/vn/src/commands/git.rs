use crate::{GitArgs, GitSubcommand};
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(args: GitArgs) -> Result<()> {
    match args.command {
        GitSubcommand::Sync { root } => sync(root),
        GitSubcommand::Status { root } => status(root),
    }
}

fn sync(root: Option<PathBuf>) -> Result<()> {
    let repos = discover_repos(root)?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(());
    }

    for repo in repos {
        let label = repo.display().to_string();
        let result = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("pull")
            .arg("--ff-only")
            .status();

        match result {
            Ok(status) if status.success() => println!("[OK] {}", label),
            Ok(status) => println!("[FAIL] {} ({})", label, status),
            Err(err) => println!("[FAIL] {} ({})", label, err),
        }
    }

    Ok(())
}

fn status(root: Option<PathBuf>) -> Result<()> {
    let repos = discover_repos(root)?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(());
    }

    for repo in repos {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["status", "--short", "--branch"])
            .output();

        println!("{}", repo.display());
        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                if text.trim().is_empty() {
                    println!("  clean");
                } else {
                    for line in text.lines() {
                        println!("  {}", line);
                    }
                }
            }
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stderr);
                println!("  failed: {}", text.trim());
            }
            Err(err) => println!("  failed: {}", err),
        }
        println!();
    }

    Ok(())
}

fn discover_repos(root: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    let root = root.unwrap_or_else(default_repos_root);

    // Validate the root path to prevent directory traversal
    validate_path_for_repo_discovery(&root)?;

    let mut repos = Vec::new();

    if is_repo(&root) {
        repos.push(root);
        return Ok(repos);
    }

    if !root.exists() {
        return Ok(repos);
    }

    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed reading root folder: {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && is_repo(&path) {
            repos.push(path);
        }
    }

    repos.sort();
    Ok(repos)
}

fn default_repos_root() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join("dev")
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

fn is_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Validate that a path is safe for repository discovery.
/// Rejects paths with ".." components to prevent directory traversal attacks.
fn validate_path_for_repo_discovery(path: &Path) -> Result<()> {
    // Reject paths containing ".." components
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(anyhow::anyhow!(
            "path traversal detected in git root: {}. Using '..' is not allowed.",
            path.display()
        ));
    }

    // Ensure the path is absolute or relative, not containing suspicious patterns
    let path_str = path.to_string_lossy();
    if path_str.contains("..") || path_str.contains("//") {
        return Err(anyhow::anyhow!(
            "suspicious path pattern in git root: {}",
            path.display()
        ));
    }

    Ok(())
}
