use std::process::Command;
use clap::Parser;
use anyhow::{Context, Result};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Parser)]
enum Cmd {
    BuildEbpf {
        #[arg(long, default_value = "bpfel-unknown-none")]
        target: String,
        #[arg(long)]
        release: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Cmd::BuildEbpf { target, release } => build_ebpf(target, release)?,
    }

    Ok(())
}

fn build_ebpf(target: String, release: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("+nightly")
        .arg("build")
        .arg("--package")
        .arg("hud-ebpf")
        .arg("--target")
        .arg(&target)
        .arg("-Z")
        .arg("build-std=core");

    if release {
        cmd.arg("--release");
    }

    let status = cmd.status().context("Failed to build eBPF program")?;

    if !status.success() {
        anyhow::bail!("Failed to build eBPF program");
    }

    println!("✓ eBPF program built successfully");
    println!("  Target: {}", target);
    println!("  Profile: {}", if release { "release" } else { "debug" });

    Ok(())
}
