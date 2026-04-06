use anyhow::{Context, Result};

use super::run_tmux;

/// Represents a tmux window
#[derive(Debug, Clone)]
pub struct TmuxWindow {
    pub index: usize,
    pub name: String,
    pub active: bool,
    pub panes: usize,
}

/// List all windows in a session
pub fn list_windows(session_name: &str) -> Result<Vec<TmuxWindow>> {
    let output = run_tmux(&[
        "list-windows",
        "-t",
        session_name,
        "-F",
        "#{window_index}\t#{window_name}\t#{window_active}\t#{window_panes}",
    ])?;

    let windows = output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                Some(TmuxWindow {
                    index: parts[0].parse().unwrap_or(0),
                    name: parts[1].to_string(),
                    active: parts[2] == "1",
                    panes: parts[3].parse().unwrap_or(1),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(windows)
}

/// Create a new window in a session
pub fn create_window(session_name: &str, window_name: &str, working_dir: Option<&str>) -> Result<()> {
    let mut args = vec!["new-window", "-t", session_name, "-n", window_name];
    if let Some(dir) = working_dir {
        args.push("-c");
        args.push(dir);
    }
    run_tmux(&args).context(format!(
        "Failed to create window '{}' in session '{}'",
        window_name, session_name
    ))?;
    Ok(())
}

/// Select (switch to) a specific window
pub fn select_window(session_name: &str, window_index: usize) -> Result<()> {
    let target = format!("{}:{}", session_name, window_index);
    run_tmux(&["select-window", "-t", &target])?;
    Ok(())
}

/// Rename a window
pub fn rename_window(session_name: &str, window_index: usize, new_name: &str) -> Result<()> {
    let target = format!("{}:{}", session_name, window_index);
    run_tmux(&["rename-window", "-t", &target, new_name])?;
    Ok(())
}

/// Kill a window
pub fn kill_window(session_name: &str, window_index: usize) -> Result<()> {
    let target = format!("{}:{}", session_name, window_index);
    run_tmux(&["kill-window", "-t", &target])?;
    Ok(())
}
