use anyhow::{Context, Result};
use clap::Parser;
use std::process::Command;

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
        Cmd::BuildEbpf { target, release } => build_ebpf(&target, release)?,
    }

    Ok(())
}

fn build_ebpf(target: &str, _release: bool) -> Result<()> {
    // Always build in release mode because debug builds with recent Rust nightlies (1.94+)
    // pull in formatting code (LowerHex) that's incompatible with BPF linker.
    // Release mode uses LTO to eliminate dead code.
    let mut cmd = Command::new("cargo");
    cmd.arg("+nightly")
        .arg("build")
        .arg("--package")
        .arg("hud-ebpf")
        .arg("--target")
        .arg(target)
        .arg("-Z")
        .arg("build-std=core")
        .arg("--release"); // Always release

    let status = cmd.status().context("Failed to build eBPF program")?;

    if !status.success() {
        anyhow::bail!("Failed to build eBPF program");
    }

    println!("✓ eBPF program built successfully");
    println!("  Target: {target}");
    println!("  Profile: release (always)");

    Ok(())
}
