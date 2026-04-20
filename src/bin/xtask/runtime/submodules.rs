use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, ensure};

pub(super) fn maybe_init_submodules(root: &Path) -> anyhow::Result<()> {
    if !root.join(".git").exists() {
        return Ok(());
    }
    let status = Command::new("rtk")
        .arg("git")
        .arg("-C")
        .arg(root)
        .args([
            "submodule",
            "update",
            "--init",
            "--recursive",
            "third_party/upstream/libkrun",
            "third_party/upstream/libkrunfw",
            "third_party/upstream/krunkit",
        ])
        .stdin(Stdio::null())
        .status()
        .context("initializing upstream submodules")?;
    if !status.success() {
        eprintln!("warning: git submodule update failed; continuing with existing checkout");
    }
    Ok(())
}

pub(super) fn ensure_upstream_checkout(
    root: &Path,
    rel: &str,
    probe: &str,
) -> anyhow::Result<PathBuf> {
    let checkout = root.join(rel);
    if checkout.join(probe).is_file() {
        return Ok(checkout);
    }
    maybe_init_submodules(root)?;
    ensure!(
        checkout.join(probe).is_file(),
        "missing {rel} checkout; run `git submodule update --init --recursive {rel}`"
    );
    Ok(checkout)
}
