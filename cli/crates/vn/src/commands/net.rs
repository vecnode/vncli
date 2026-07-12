use crate::{NetArgs, NetSubcommand};
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::process::{Command, Stdio};

/// Common TCP ports used for the fast subnet sweep. RustScan does no host
/// discovery, so scanning all 65535 ports across a /24 would spend minutes
/// timing out on dead hosts. Restricting the subnet sweep to these ports keeps
/// it responsive while still covering the services that usually matter (incl.
/// 11434 for Ollama, which this project uses).
const COMMON_PORTS: &str = "21,22,23,25,53,80,110,111,135,139,143,443,445,587,\
993,995,1433,1723,3000,3306,3389,5000,5432,5900,6379,8000,8080,8443,8888,9000,\
9090,11434,27017";

pub fn run(args: NetArgs) -> Result<()> {
    match args.command.unwrap_or(NetSubcommand::Scan { target: None }) {
        NetSubcommand::Scan { target } => scan(target),
    }
}

/// Scan open ports using RustScan (https://github.com/bee-san/RustScan).
///
/// With no target this scans the host's local /24 subnet over a common-port
/// set so live hosts surface in seconds. Pass an explicit target (IP, CIDR, or
/// hostname) to run RustScan's default full-port scan against it.
fn scan(target: Option<String>) -> Result<()> {
    let (addresses, is_subnet) = match target {
        Some(t) => (t, false),
        None => (default_subnet_target()?, true),
    };

    if !rustscan_available() {
        eprintln!("[ERROR] rustscan was not found on PATH.");
        eprintln!("[INFO] Install it with: cargo install rustscan");
        eprintln!(
            "[INFO] The vncli launchers (run_cli.bat / run_cli.sh) install it automatically."
        );
        bail!("rustscan is not installed");
    }

    // `--greppable` keeps RustScan self-contained (it prints "ip -> [ports]"
    // and does not hand off to nmap); `--scripts none` makes that explicit so
    // nmap is never required. `--tries 1` plus a short timeout makes dead hosts
    // fail fast instead of stalling the sweep.
    let mut args: Vec<String> = vec![
        "-a".into(),
        addresses.clone(),
        "--greppable".into(),
        "--scripts".into(),
        "none".into(),
        "--tries".into(),
        "1".into(),
    ];
    if is_subnet {
        args.push("--timeout".into());
        args.push("800".into());
        args.push("-p".into());
        args.push(COMMON_PORTS.into());
    } else {
        args.push("--timeout".into());
        args.push("1500".into());
    }

    println!("[Open Port Scan]");
    println!("[INFO] Target: {}", addresses);
    if is_subnet {
        println!(
            "[INFO] Scope: common ports only (fast subnet sweep; RustScan has no host discovery)."
        );
        println!(
            "[INFO] Tip: pass an explicit IP/host (vn net scan <host>) for a full 65535-port scan."
        );
    }
    println!("[INFO] Running: rustscan {}", args.join(" "));
    println!();
    let _ = std::io::stdout().flush();

    let status = Command::new("rustscan")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run rustscan")?;

    if !status.success() {
        bail!("rustscan exited with status: {}", status);
    }

    println!();
    println!("[INFO] Scan complete.");
    let _ = std::io::stdout().flush();
    Ok(())
}

fn default_subnet_target() -> Result<String> {
    let ip = local_ipv4().context("could not determine local IPv4 address")?;
    let o = ip.octets();
    Ok(format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
}

/// Determine the primary local IPv4 address by opening a UDP socket toward a
/// public address. Connecting a UDP socket does not send any packets; it only
/// makes the OS pick the outbound interface, whose address we then read back.
/// This works the same on Windows and Linux without extra dependencies.
fn local_ipv4() -> Result<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").context("failed to bind local UDP socket")?;
    socket
        .connect("1.1.1.1:80")
        .context("failed to resolve outbound network interface")?;

    match socket.local_addr()?.ip() {
        IpAddr::V4(v4) => Ok(v4),
        IpAddr::V6(_) => bail!("local address is IPv6; pass an explicit target to 'vn net scan'"),
    }
}

fn rustscan_available() -> bool {
    Command::new("rustscan")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
