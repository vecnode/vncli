//! Native Docker app management: one cross-platform code path that replaces
//! the old per-OS run_*/stop_* script pairs.
//!
//! Every app is described by an [`AppPlan`] built in [`plan_for`]; the shared
//! engine handles docker checks, optional image build, container lifecycle,
//! loopback-only port publishing, hardening flags, readiness wait, and opening
//! the browser. Adding an app = adding one arm to `plan_for` (and a menu item).

use crate::config::LoadedConfig;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const APP_NAMES: &[&str] = &[
    "docs",
    "silverbullet",
    "bentopdf",
    "library",
    "media-downloader",
    "doc-processor",
];

enum Lifecycle {
    /// Rebuild/repull and replace the container on every open (`rm_on_exit`
    /// adds `--rm` so a stopped container disappears).
    Recreate { rm_on_exit: bool },
    /// Reuse a running container; `docker start` a stopped one; run otherwise.
    Reuse,
}

struct AppPlan {
    container: &'static str,
    image: String,
    /// (build context, optional -f dockerfile) — None means a pulled image.
    build: Option<(PathBuf, Option<PathBuf>)>,
    lifecycle: Lifecycle,
    /// Apply --cap-drop ALL --security-opt no-new-privileges (+ pids limit).
    /// True for our locally built images (they run non-root and need no caps);
    /// false for pulled vendor images whose entrypoints legitimately need
    /// privilege transitions (e.g. a root-to-app-user setpriv step requiring
    /// CAP_SETUID/SETGID).
    harden: bool,
    pids_limit: Option<u32>,
    /// Linux: pass --user $(uid):$(gid) so bind-mount files stay user-owned.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_user: bool,
    /// Host port -> container port, always published on 127.0.0.1.
    ports: Vec<(u16, u16)>,
    env: Vec<(String, String)>,
    /// Host path -> container path (host dirs are created if missing).
    mounts: Vec<(PathBuf, String)>,
    /// Port to poll for readiness and how many 1.5s attempts to make.
    wait_port: u16,
    wait_tries: u32,
    open_url: String,
    info: Vec<String>,
}

fn plan_for(name: &str, loaded: &LoadedConfig) -> Result<AppPlan> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    let desktop = home.join("Desktop");

    let plan = match name {
        "docs" => {
            let root = repo_root(loaded)?;
            AppPlan {
                container: "vncli-docs",
                image: "vncli-docs:latest".into(),
                build: Some((
                    root.join("docs"),
                    Some(root.join("docs").join("Dockerfile")),
                )),
                lifecycle: Lifecycle::Recreate { rm_on_exit: false },
                harden: true,
                pids_limit: Some(512),
                linux_user: false,
                ports: vec![(3000, 3000)],
                env: vec![],
                mounts: vec![],
                wait_port: 3000,
                wait_tries: 20,
                open_url: "http://localhost:3000".into(),
                info: vec![],
            }
        }
        "silverbullet" => AppPlan {
            container: "silverbullet",
            image: "ghcr.io/silverbulletmd/silverbullet:latest".into(),
            build: None,
            lifecycle: Lifecycle::Recreate { rm_on_exit: true },
            harden: false,
            pids_limit: None,
            linux_user: false,
            ports: vec![(3000, 3000)],
            env: vec![("SB_USER".into(), "user:password".into())],
            mounts: vec![(home.join("silverbullet-space"), "/space".into())],
            wait_port: 3000,
            wait_tries: 20,
            open_url: "http://localhost:3000".into(),
            info: vec![
                "Username: user  Password: password (change SB_USER before wider exposure)".into(),
                format!("Data folder: {}", home.join("silverbullet-space").display()),
            ],
        },
        "bentopdf" => AppPlan {
            container: "bentopdf",
            // Self-hosted build (AGPL-3.0, free) - not the commercial build.
            image: "ghcr.io/alam00000/bentopdf-simple:latest".into(),
            build: None,
            lifecycle: Lifecycle::Reuse,
            harden: false,
            pids_limit: None,
            linux_user: false,
            ports: vec![(8080, 8080)],
            env: vec![],
            mounts: vec![],
            wait_port: 8080,
            wait_tries: 40,
            open_url: "http://localhost:8080".into(),
            info: vec!["Self-hosted PDF toolkit (AGPL-3.0 build).".into()],
        },
        "library" => {
            let root = repo_root(loaded)?;
            AppPlan {
                container: "library",
                image: "vncli-library".into(),
                build: Some((root.join("docker").join("library-portal"), None)),
                lifecycle: Lifecycle::Recreate { rm_on_exit: false },
                harden: true,
                pids_limit: Some(512),
                linux_user: true,
                ports: vec![(8090, 8090)],
                env: vec![],
                mounts: vec![(root.join("library"), "/library".into())],
                wait_port: 8090,
                wait_tries: 20,
                open_url: "http://localhost:8090".into(),
                info: vec![
                    "No PDFs are copied into the image; state lives in library/.portal/.".into(),
                ],
            }
        }
        "media-downloader" => {
            let root = repo_root(loaded)?;
            AppPlan {
                container: "media-downloader",
                image: "vncli-media-downloader".into(),
                build: Some((root.join("docker").join("media-downloader"), None)),
                lifecycle: Lifecycle::Recreate { rm_on_exit: false },
                harden: true,
                pids_limit: Some(512),
                linux_user: true,
                ports: vec![(8095, 8095)],
                env: vec![("OUTPUT_LABEL".into(), "Desktop".into())],
                mounts: vec![(desktop.clone(), "/output".into())],
                wait_port: 8095,
                wait_tries: 20,
                open_url: "http://localhost:8095".into(),
                info: vec!["Downloads are saved to your Desktop.".into()],
            }
        }
        "doc-processor" => {
            let root = repo_root(loaded)?;
            AppPlan {
                container: "vncli-doc-processor",
                image: "vncli-doc-processor:latest".into(),
                build: Some((
                    root.clone(),
                    Some(
                        root.join("docker")
                            .join("media-processor")
                            .join("Dockerfile"),
                    ),
                )),
                lifecycle: Lifecycle::Recreate { rm_on_exit: true },
                harden: true,
                pids_limit: Some(512),
                linux_user: true,
                ports: vec![(8085, 8085), (8086, 8086)],
                env: vec![("HOST_DESKTOP_DIR".into(), "/host/Desktop".into())],
                mounts: vec![(desktop.clone(), "/host/Desktop".into())],
                wait_port: 8086,
                wait_tries: 20,
                open_url: "http://localhost:8085".into(),
                info: vec!["API: http://localhost:8086  (PDF output goes to your Desktop)".into()],
            }
        }
        other => bail!("unknown app: {other}. Available: {}", APP_NAMES.join(", ")),
    };
    Ok(plan)
}

/// The default CLI/TUI reporter: prints each progress line to stdout exactly
/// as `open`/`stop` always have. MCP tool wrappers pass a different reporter
/// (capturing lines into the tool-call result, or discarding them) instead of
/// `println!`-ing straight to the process's real stdout, which would corrupt
/// an MCP stdio transport's JSON-RPC stream.
pub fn println_reporter(line: &str) {
    println!("{line}");
}

pub fn open(name: &str, loaded: &LoadedConfig, no_open: bool) -> Result<()> {
    open_reported(name, loaded, no_open, &mut println_reporter)
}

pub fn open_reported(
    name: &str,
    loaded: &LoadedConfig,
    no_open: bool,
    report: &mut dyn FnMut(&str),
) -> Result<()> {
    let plan = plan_for(name, loaded)?;
    check_docker_ready(report)?;

    // Per-app preparation.
    if name == "silverbullet" {
        backup_silverbullet_space(report)?;
    }
    for (host_path, _) in &plan.mounts {
        fs::create_dir_all(host_path)
            .with_context(|| format!("failed to create mount dir: {}", host_path.display()))?;
    }

    if let Some((context, dockerfile)) = &plan.build {
        report(&format!(
            "[DOCKER] [INFO] Building image '{}'...",
            plan.image
        ));
        let mut args: Vec<String> = vec!["build".into(), "-t".into(), plan.image.clone()];
        if let Some(df) = dockerfile {
            args.push("-f".into());
            args.push(df.display().to_string());
        }
        args.push(context.display().to_string());
        run_docker_streaming(&args, report)?;
        report("[DOCKER] [OK] Image built.");
    }

    match plan.lifecycle {
        Lifecycle::Recreate { rm_on_exit } => {
            let _ = docker_quiet(&["rm", "-f", plan.container]);
            run_container(&plan, rm_on_exit)?;
            report(&format!(
                "[DOCKER] [OK] Container started: {}",
                plan.container
            ));
        }
        Lifecycle::Reuse => {
            let state = container_state(plan.container)?;
            if state == ContainerState::Running {
                report(&format!(
                    "[DOCKER] [OK] Container '{}' is already running.",
                    plan.container
                ));
            } else if state == ContainerState::Stopped && container_exit_code(plan.container)? == 0
            {
                report(&format!(
                    "[DOCKER] [INFO] Starting existing container '{}'...",
                    plan.container
                ));
                docker_quiet(&["start", plan.container])
                    .with_context(|| format!("failed to start container {}", plan.container))?;
            } else {
                if state == ContainerState::Stopped {
                    report(&format!(
                        "[DOCKER] [INFO] Container '{}' previously exited with an error; recreating it...",
                        plan.container
                    ));
                    let _ = docker_quiet(&["rm", "-f", plan.container]);
                }
                report(&format!(
                    "[DOCKER] [INFO] Running image '{}'. First run downloads it; this can take a while...",
                    plan.image
                ));
                run_container(&plan, false)?;
                report(&format!(
                    "[DOCKER] [OK] Container started: {}",
                    plan.container
                ));
            }
        }
    }

    report(&format!(
        "[DOCKER] [INFO] Waiting for {} ...",
        plan.open_url
    ));
    if wait_ready(&plan)? {
        report(&format!("[DOCKER] [OK] {} is ready.", name));
    } else {
        report(&format!(
            "[DOCKER] [WARNING] {} did not respond yet; opening the browser anyway.",
            name
        ));
    }

    if no_open {
        report("[DOCKER] [INFO] --no-open set; not launching a browser.");
    } else {
        open_browser(&plan.open_url, report);
    }

    report("");
    report(&format!("[DOCKER] [INFO] Open:  {}", plan.open_url));
    for line in &plan.info {
        report(&format!("[DOCKER] [INFO] {}", line));
    }
    report(&format!(
        "[DOCKER] [INFO] Stop:  vn app stop {}   (or: docker stop {})",
        name, plan.container
    ));
    report(&format!(
        "[DOCKER] [INFO] Logs:  docker logs -f {}",
        plan.container
    ));
    Ok(())
}

pub fn stop(name: &str, loaded: &LoadedConfig) -> Result<()> {
    stop_reported(name, loaded, &mut println_reporter)
}

pub fn stop_reported(
    name: &str,
    loaded: &LoadedConfig,
    report: &mut dyn FnMut(&str),
) -> Result<()> {
    let plan = plan_for(name, loaded)?;
    check_docker_available()?;

    if container_state(plan.container)? == ContainerState::Absent {
        report(&format!(
            "[DOCKER] [INFO] No '{}' container exists. Nothing to stop.",
            plan.container
        ));
        return Ok(());
    }
    report(&format!("[DOCKER] [INFO] Stopping '{}'...", plan.container));
    docker_quiet(&["stop", plan.container])
        .with_context(|| format!("failed to stop container {}", plan.container))?;
    report(&format!("[DOCKER] [OK] Stopped '{}'.", plan.container));
    Ok(())
}

pub fn list() -> Result<()> {
    println!("Available apps (vn app open|stop <name>):");
    for name in APP_NAMES {
        println!("  {}", name);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// read-only docker introspection (used by `vn mcp`'s docker toolset)
// ---------------------------------------------------------------------------

/// One row of `docker ps -a`, as seen by [`docker_ps_all`].
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
}

/// List every container docker knows about (running or not) - not just the
/// ones vncli manages, since the point is to answer "what does docker
/// actually have" rather than re-describe [`APP_NAMES`].
pub fn docker_ps_all() -> Result<Vec<ContainerInfo>> {
    check_docker_available()?;
    let output = Command::new("docker")
        .args(["ps", "-a", "--format", "{{json .}}"])
        .stdin(Stdio::null())
        .output()
        .context("failed to run: docker ps -a")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("docker ps -a failed: {}", stderr.trim());
    }

    let mut containers = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("failed to parse docker ps output: {line}"))?;
        let field = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        containers.push(ContainerInfo {
            id: field("ID"),
            name: field("Names"),
            image: field("Image"),
            state: field("State"),
            status: field("Status"),
            ports: field("Ports"),
        });
    }
    Ok(containers)
}

/// Tail a container's logs (stdout+stderr combined, in that order).
pub fn docker_logs_tail(name: &str, lines: u32) -> Result<String> {
    check_docker_available()?;
    let output = Command::new("docker")
        .args(["logs", "--tail", &lines.to_string(), name])
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run: docker logs {name}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("docker logs {name} failed: {}", stderr.trim());
    }
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(text)
}

// ---------------------------------------------------------------------------
// docker maintenance (vn docker check|stop-all|remove-containers|remove-images)
// ---------------------------------------------------------------------------

pub fn docker_check() -> Result<()> {
    docker_check_reported(&mut println_reporter)
}

/// Report-based twin of [`docker_check`] - MCP tool wrappers pass a reporter
/// that captures lines instead of `println!`-ing straight to stdout (which,
/// unlike the CLI, would corrupt an MCP stdio transport's JSON-RPC stream).
/// Captures `docker ps`'s own output instead of inheriting stdio for the
/// same reason.
pub fn docker_check_reported(report: &mut dyn FnMut(&str)) -> Result<()> {
    check_docker_ready(report)?;
    let output = Command::new("docker")
        .arg("ps")
        .stdin(Stdio::null())
        .output()
        .context("failed to run docker ps")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "docker ps exited with status {}: {}",
            output.status,
            stderr.trim()
        );
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        report(line);
    }
    let containers = docker_lines(&["ps", "-aq"])?.len();
    let images = docker_lines(&["images", "-aq"])?.len();
    report(&format!("Containers: {containers}"));
    report(&format!("Images: {images}"));
    Ok(())
}

pub fn docker_stop_all() -> Result<()> {
    docker_stop_all_reported(&mut println_reporter)
}

pub fn docker_stop_all_reported(report: &mut dyn FnMut(&str)) -> Result<()> {
    check_docker_ready(report)?;
    let running = docker_lines(&["ps", "-q"])?;
    if running.is_empty() {
        report("[DOCKER] [INFO] No running containers to stop.");
        return Ok(());
    }
    report("[DOCKER] [INFO] Stopping all running containers...");
    for id in &running {
        let _ = docker_quiet(&["stop", id]);
    }
    report("[DOCKER] [OK] All running containers stopped.");
    Ok(())
}

pub fn docker_remove_containers() -> Result<()> {
    docker_remove_containers_reported(&mut println_reporter)
}

pub fn docker_remove_containers_reported(report: &mut dyn FnMut(&str)) -> Result<()> {
    check_docker_ready(report)?;
    docker_stop_all_reported(report)?;
    let all = docker_lines(&["ps", "-aq"])?;
    if all.is_empty() {
        report("[DOCKER] [INFO] No containers to remove.");
        return Ok(());
    }
    report("[DOCKER] [INFO] Removing all containers...");
    for id in &all {
        let _ = docker_quiet(&["rm", "-f", id]);
    }
    report("[DOCKER] [OK] All containers removed.");
    Ok(())
}

pub fn docker_remove_images() -> Result<()> {
    docker_remove_images_reported(&mut println_reporter)
}

pub fn docker_remove_images_reported(report: &mut dyn FnMut(&str)) -> Result<()> {
    check_docker_ready(report)?;
    let images = docker_lines(&["images", "-aq"])?;
    if images.is_empty() {
        report("[DOCKER] [INFO] No images to remove.");
        return Ok(());
    }
    report("[DOCKER] [INFO] Removing all Docker images...");
    for id in &images {
        let _ = docker_quiet(&["rmi", "-f", id]);
    }
    report("[DOCKER] [OK] All images removed.");
    Ok(())
}

/// Wraps `docker system df` - a human-readable breakdown of disk space used
/// by images, containers, and build cache.
pub fn docker_disk_usage() -> Result<String> {
    check_docker_available()?;
    let output = Command::new("docker")
        .args(["system", "df"])
        .stdin(Stdio::null())
        .output()
        .context("failed to run: docker system df")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("docker system df failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// engine internals
// ---------------------------------------------------------------------------

fn run_container(plan: &AppPlan, rm_on_exit: bool) -> Result<()> {
    let mut args: Vec<String> = vec!["run".into(), "-d".into()];
    if rm_on_exit {
        args.push("--rm".into());
    }
    args.push("--name".into());
    args.push(plan.container.into());

    // Security posture for our own images (see SECURITY.md). Pulled vendor
    // images run as upstream intends - their entrypoints may need caps.
    if plan.harden {
        args.push("--cap-drop".into());
        args.push("ALL".into());
        args.push("--security-opt".into());
        args.push("no-new-privileges".into());
        if let Some(limit) = plan.pids_limit {
            args.push("--pids-limit".into());
            args.push(limit.to_string());
        }
    }
    #[cfg(target_os = "linux")]
    if plan.linux_user {
        args.push("--user".into());
        args.push(format!("{}:{}", uid(), gid()));
    }

    for (host, cont) in &plan.ports {
        args.push("-p".into());
        args.push(format!("127.0.0.1:{host}:{cont}"));
    }
    for (key, value) in &plan.env {
        args.push("-e".into());
        args.push(format!("{key}={value}"));
    }
    for (host_path, cont_path) in &plan.mounts {
        args.push("-v".into());
        args.push(format!("{}:{}", host_path.display(), cont_path));
    }
    args.push(plan.image.clone());

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    docker_quiet(&arg_refs).with_context(|| format!("docker run failed for {}", plan.container))
}

#[derive(PartialEq)]
enum ContainerState {
    Running,
    Stopped,
    Absent,
}

fn container_state(name: &str) -> Result<ContainerState> {
    let filter = format!("name=^/{name}$");
    if !docker_lines(&["ps", "-q", "--filter", &filter])?.is_empty() {
        return Ok(ContainerState::Running);
    }
    if !docker_lines(&["ps", "-aq", "--filter", &filter])?.is_empty() {
        return Ok(ContainerState::Stopped);
    }
    Ok(ContainerState::Absent)
}

fn check_docker_available() -> Result<()> {
    let ok = Command::new("docker")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        bail!("Docker is not available or not in PATH. Install Docker: https://docs.docker.com/engine/install/");
    }
    Ok(())
}

fn check_docker_ready(report: &mut dyn FnMut(&str)) -> Result<()> {
    check_docker_available()?;
    let ok = Command::new("docker")
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        bail!("Docker daemon is not running. Start Docker and try again.");
    }
    report("[DOCKER] [OK] Docker daemon is running.");
    Ok(())
}

fn docker_quiet(args: &[&str]) -> Result<()> {
    let output = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run: docker {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("docker {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(())
}

fn docker_lines(args: &[&str]) -> Result<Vec<String>> {
    let output = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run: docker {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("docker {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Run `docker <args>` (used only for `docker build`), streaming its output
/// line-by-line through `report` as it's produced. Piped (not inherited) so
/// the build's own stdout/stderr never touch the process's real stdio -
/// harmless for the plain CLI (the reporter still `println!`s each line as it
/// arrives, so it looks the same as inheriting), but required for MCP: an
/// inherited stdout here would corrupt an MCP stdio transport's JSON-RPC
/// stream mid-build.
fn run_docker_streaming(args: &[String], report: &mut dyn FnMut(&str)) -> Result<()> {
    let mut child = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run: docker {}", args.join(" ")))?;

    let (tx, rx) = mpsc::channel::<String>();

    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    for line in rx {
        report(&line);
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait on: docker {}", args.join(" ")))?;
    if !status.success() {
        bail!("docker {} exited with status: {status}", args[0]);
    }
    Ok(())
}

/// Wait until the app answers HTTP on its port. A bare TCP connect is not
/// enough on Docker Desktop: the port proxy accepts connections even when the
/// process inside is dead. Also fail fast (with the log tail) if the
/// container exits while we wait.
fn wait_ready(plan: &AppPlan) -> Result<bool> {
    for _ in 0..plan.wait_tries {
        if container_state(plan.container)? != ContainerState::Running {
            let logs = docker_logs_tail(plan.container, 15).unwrap_or_default();
            bail!(
                "container '{}' exited during startup. Last log lines:
{}",
                plan.container,
                logs.trim()
            );
        }
        if http_probe(plan.wait_port) {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
    Ok(false)
}

/// Minimal HTTP GET: ready only if the server sends back an HTTP response.
fn http_probe(port: u16) -> bool {
    use std::io::{Read, Write};
    let addr = ([127, 0, 0, 1], port).into();
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream
        .write_all(
            b"GET / HTTP/1.1
Host: localhost
Connection: close

",
        )
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 5];
    match stream.read_exact(&mut buf) {
        Ok(()) => &buf == b"HTTP/",
        Err(_) => false,
    }
}

fn container_exit_code(name: &str) -> Result<i64> {
    let lines = docker_lines(&["inspect", "-f", "{{.State.ExitCode}}", name])?;
    Ok(lines.first().and_then(|l| l.parse().ok()).unwrap_or(0))
}

fn open_browser(url: &str, report: &mut dyn FnMut(&str)) {
    #[cfg(target_os = "windows")]
    {
        let chrome_candidates = [
            std::env::var("ProgramFiles").ok().map(|p| {
                PathBuf::from(p)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe")
            }),
            std::env::var("ProgramFiles(x86)").ok().map(|p| {
                PathBuf::from(p)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe")
            }),
            std::env::var("LOCALAPPDATA").ok().map(|p| {
                PathBuf::from(p)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe")
            }),
        ];
        for candidate in chrome_candidates.into_iter().flatten() {
            if candidate.exists() {
                report(&format!("[DOCKER] [INFO] Opening Chrome at {url}"));
                let _ = Command::new(candidate)
                    .arg(url)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                return;
            }
        }
        report(&format!(
            "[DOCKER] [INFO] Chrome not found; opening default browser at {url}"
        ));
        let _ = Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        for browser in [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ] {
            if Command::new("which")
                .arg(browser)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                report(&format!("[DOCKER] [INFO] Opening Chrome at {url}"));
                let _ = Command::new(browser)
                    .arg(url)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                return;
            }
        }
        report(&format!(
            "[DOCKER] [INFO] Chrome not found; opening default browser at {url}"
        ));
        let _ = Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        report(&format!("[DOCKER] [INFO] Open manually: {url}"));
    }
}

/// Back up ~/silverbullet-space to Desktop/silverbullet-space-backup-<ts>
/// before recreating the container (matches the old win11 script; the old
/// ubuntu script asked interactively — now both platforms back up always).
fn backup_silverbullet_space(report: &mut dyn FnMut(&str)) -> Result<()> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    let space = home.join("silverbullet-space");
    if !space.exists() {
        report("[DOCKER] [INFO] Space folder does not exist, creating it.");
        fs::create_dir_all(&space)?;
        return Ok(());
    }
    let desktop = home.join("Desktop");
    if !desktop.exists() {
        report("[DOCKER] [WARNING] No Desktop folder; skipping space backup.");
        return Ok(());
    }
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let target = desktop.join(format!("silverbullet-space-backup-{ts}"));
    report(&format!(
        "[DOCKER] [INFO] Backing up space folder to: {}",
        target.display()
    ));
    copy_dir_recursive(&space, &target)?;
    report(&format!(
        "[DOCKER] [OK] Backup completed: {}",
        target.display()
    ));
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("failed to copy {}", src_path.display()))?;
        }
    }
    Ok(())
}

fn repo_root(loaded: &LoadedConfig) -> Result<PathBuf> {
    crate::commands::run::detect_repo_root(loaded)
}

#[cfg(target_os = "linux")]
fn uid() -> u32 {
    // Read our own uid/gid from /proc (no libc dependency needed).
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(1000)
}

#[cfg(target_os = "linux")]
fn gid() -> u32 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Gid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(1000)
}
