use anyhow::{Context, Result};

use super::run_tmux;

/// Represents a tmux session
#[derive(Debug, Clone)]
pub struct TmuxSession {
    pub name: String,
    pub window_count: usize,
    pub attached: bool,
    pub created: Option<String>,
}

/// List all tmux sessions
pub fn list_sessions() -> Result<Vec<TmuxSession>> {
    if !super::is_server_running() {
        return Ok(vec![]);
    }

    let output = run_tmux(&[
        "list-sessions",
        "-F",
        "#{session_name}\t#{session_windows}\t#{session_attached}\t#{session_created_string}",
    ])?;

    let sessions = output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                Some(TmuxSession {
                    name: parts[0].to_string(),
                    window_count: parts[1].parse().unwrap_or(0),
                    attached: parts[2] == "1",
                    created: parts.get(3).map(|s| s.to_string()),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(sessions)
}

/// Check if a session with the given name exists
pub fn session_exists(name: &str) -> Result<bool> {
    let result = run_tmux(&["has-session", "-t", name]);
    Ok(result.is_ok())
}

/// Create a new tmux session
pub fn create_session(name: &str, working_dir: Option<&str>) -> Result<()> {
    let mut args = vec!["new-session", "-d", "-s", name];
    if let Some(dir) = working_dir {
        args.push("-c");
        args.push(dir);
    }
    run_tmux(&args).context(format!("Failed to create session '{}'", name))?;
    Ok(())
}

/// Switch to a session (if inside tmux) or attach (if outside)
pub fn switch_session(name: &str) -> Result<()> {
    if super::is_inside_tmux() {
        run_tmux(&["switch-client", "-t", name])?;
    } else {
        run_tmux(&["attach-session", "-t", name])?;
    }
    Ok(())
}

/// Kill a session
pub fn kill_session(name: &str) -> Result<()> {
    run_tmux(&["kill-session", "-t", name])?;
    Ok(())
}

/// Rename a session
pub fn rename_session(old_name: &str, new_name: &str) -> Result<()> {
    run_tmux(&["rename-session", "-t", old_name, new_name])?;
    Ok(())
}

/// Get the name of the currently active session
pub fn current_session_name() -> Result<String> {
    run_tmux(&["display-message", "-p", "#{session_name}"])
}
