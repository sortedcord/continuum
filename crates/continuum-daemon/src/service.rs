//! `continuum service`: install the daemon as a systemd user service.

use std::path::PathBuf;

use anyhow::Context as _;
use tokio::process::Command;

const UNIT_NAME: &str = "continuum.service";
const TEMPLATE: &str = include_str!("../../../packaging/systemd/continuum.service");

fn unit_path() -> anyhow::Result<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(config.join("systemd/user").join(UNIT_NAME))
}

async fn systemctl(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .await?;
    anyhow::ensure!(
        status.success(),
        "systemctl --user {} failed",
        args.join(" ")
    );
    Ok(())
}

/// Writes the unit (pointing at this binary) and enables it.
pub async fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let unit = TEMPLATE.replace("@CONTINUUM@", &exe.to_string_lossy());
    let path = unit_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, unit)?;
    systemctl(&["daemon-reload"]).await?;
    systemctl(&["enable", "--now", UNIT_NAME]).await?;
    println!("Installed {} and started it.", path.display());
    println!("Logs: journalctl --user -u continuum -f");
    Ok(())
}

pub async fn uninstall() -> anyhow::Result<()> {
    let path = unit_path()?;
    systemctl(&["disable", "--now", UNIT_NAME]).await.ok();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    systemctl(&["daemon-reload"]).await?;
    println!("Removed {}.", path.display());
    Ok(())
}
