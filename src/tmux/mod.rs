mod session;
mod window;

pub use session::*;
pub use window::*;

use anyhow::{bail, Context, Result};
use std::process::Command;

/// Run a tmux command and return stdout
fn run_tmux(args: &[&str]) -> Result<String> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .context("Failed to execute tmux command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("tmux {} failed: {}", args.join(" "), stderr.trim());
    }
}

/// Run a tmux command, ignoring failures (returns Ok(()) always)
fn run_tmux_quiet(args: &[&str]) -> Result<()> {
    let _ = Command::new("tmux").args(args).output();
    Ok(())
}

/// Check if tmux server is running
pub fn is_server_running() -> bool {
    Command::new("tmux")
        .args(["list-sessions"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if we're currently inside a tmux session
pub fn is_inside_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Send keys to a specific pane
pub fn send_keys(target: &str, keys: &str) -> Result<()> {
    run_tmux(&["send-keys", "-t", target, keys, "Enter"])?;
    Ok(())
}
