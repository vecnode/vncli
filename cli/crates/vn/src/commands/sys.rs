use crate::{SysArgs, SysSubcommand};
use anyhow::Result;
use sysinfo::{Disks, System};

pub fn run(args: SysArgs) -> Result<()> {
    match args.command.unwrap_or(SysSubcommand::Info) {
        SysSubcommand::Info => info(),
        SysSubcommand::Update => {
            println!("vn sys update is reserved for host-specific update workflows.");
            println!("Use vn run ubuntu22 or vn run win11 for script-driven updates.");
            Ok(())
        }
        SysSubcommand::Clean => {
            println!("vn sys clean is reserved for safe cleanup workflows.");
            println!("Use vn docker prune for Docker cleanup right now.");
            Ok(())
        }
    }
}

fn info() -> Result<()> {
    let mut system = System::new_all();
    system.refresh_all();

    let disks = Disks::new_with_refreshed_list();
    let total_disk: u64 = disks.iter().map(|d| d.total_space()).sum();
    let available_disk: u64 = disks.iter().map(|d| d.available_space()).sum();

    let os_name = System::name().unwrap_or_else(|| "unknown".to_string());
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "unknown".to_string());
    let host_name = System::host_name().unwrap_or_else(|| "unknown".to_string());

    println!("System Information");
    println!("------------------");
    println!("Host: {}", host_name);
    println!("OS: {} {}", os_name, os_version);
    println!("Kernel: {}", kernel);
    println!("CPU Cores: {}", system.cpus().len());
    println!("RAM Total: {}", format_bytes(system.total_memory()));
    println!("RAM Used: {}", format_bytes(system.used_memory()));
    println!("Disk Total: {}", format_bytes(total_disk));
    println!("Disk Available: {}", format_bytes(available_disk));

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}
