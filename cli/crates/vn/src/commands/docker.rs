use crate::config::LoadedConfig;
use crate::{DockerArgs, DockerSubcommand};
use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

pub fn run(args: DockerArgs, loaded: &LoadedConfig) -> Result<()> {
    match args.command {
        DockerSubcommand::Ps => run_cmd("docker", &["ps"]),
        DockerSubcommand::Prune => run_cmd("docker", &["system", "prune", "-af"]),
        DockerSubcommand::Check => crate::commands::apps::docker_check(),
        DockerSubcommand::StopAll => crate::commands::apps::docker_stop_all(),
        DockerSubcommand::RemoveContainers => crate::commands::apps::docker_remove_containers(),
        DockerSubcommand::RemoveImages => crate::commands::apps::docker_remove_images(),
        DockerSubcommand::Up { service } => {
            if matches!(service.as_deref(), Some("silverbullet")) {
                crate::commands::apps::open("silverbullet", loaded, false)
            } else if let Some(name) = service {
                validate_docker_service_name(&name)?;
                run_cmd("docker", &["start", &name])
            } else {
                println!("Use: vn docker up <service>");
                Ok(())
            }
        }
        DockerSubcommand::Down { service } => {
            if let Some(name) = service {
                validate_docker_service_name(&name)?;
                run_cmd("docker", &["stop", &name])
            } else {
                println!("Use: vn docker down <service>");
                Ok(())
            }
        }
    }
}

fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to run: {} {}", program, args.join(" ")))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("command exited with status: {}", status))
    }
}

/// Validate Docker service name to prevent injection and enforce Docker naming rules.
/// Docker allows alphanumerics, underscores, periods, and hyphens.
fn validate_docker_service_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("docker service name cannot be empty"));
    }

    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(anyhow!(
            "invalid docker service name '{}': only alphanumerics, underscore, hyphen, and period allowed",
            name
        ));
    }

    Ok(())
}
