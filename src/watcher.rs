use anyhow::{bail, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::config;
use crate::store::Store;

/// Supported watcher types
#[derive(Debug, Clone)]
pub enum WatcherType {
    /// Watch a pipeline build for completion/failure
    Pipeline { build_id: u64 },
    /// Watch a PR for merge or abandon
    PrMerge { pr_id: u64 },
    /// Watch a PR for new review comments
    PrComments { pr_id: u64, known_count: u64 },
    /// Watch a SonarQube quality gate
    Sonarqube { project_key: String, pr_id: Option<u64> },
    /// Run a custom script that exits 0 to trigger notification
    Custom { script: String },
}

/// Run a watcher loop (called in the detached child process)
pub fn run_watcher(
    watcher_type: &str,
    args: &WatcherArgs,
    interval_secs: u64,
) -> Result<()> {
    let store = Store::open()?;
    let cfg = config::AppConfig::load()?;
    let azdo = cfg.azdo.as_ref();

    let watcher_id = match &args.name {
        Some(name) => name.clone(),
        None => format!("{}-{}", watcher_type, std::process::id()),
    };

    // Build restart command for persistent watchers
    let restart_args = build_restart_args(watcher_type, args, interval_secs);

    // Register in SQLite
    store.save_watcher(&crate::store::Watcher {
        id: watcher_id.clone(),
        watcher_type: watcher_type.to_string(),
        config: format!("{:?}", args),
        pid: Some(std::process::id()),
        status: "running".to_string(),
        started_at: String::new(),
        last_check_at: None,
        last_output: None,
        persistent: args.persistent,
        restart_args: Some(restart_args),
    })?;

    let result = match watcher_type {
        "pipeline" => watch_pipeline(&store, azdo, args, &watcher_id, interval_secs),
        "pr-merge" => watch_pr_merge(&store, azdo, args, &watcher_id, interval_secs),
        "pr-comments" => watch_pr_comments(&store, azdo, args, &watcher_id, interval_secs),
        "sonarqube" => watch_sonarqube(&store, &cfg, args, &watcher_id, interval_secs),
        "custom" => watch_custom(&store, args, &watcher_id, interval_secs),
        _ => bail!("Unknown watcher type: {}", watcher_type),
    };

    if args.persistent {
        store.update_watcher_status(&watcher_id, "stopped").ok();
    } else {
        // Ephemeral: auto-delete from DB
        store.delete_watcher(&watcher_id).ok();
    }
    result
}

/// Build the CLI args needed to restart this watcher
fn build_restart_args(watcher_type: &str, args: &WatcherArgs, interval: u64) -> String {
    let mut parts = vec![
        "watch".to_string(),
        watcher_type.to_string(),
        "--interval".to_string(),
        interval.to_string(),
    ];
    if let Some(id) = args.id {
        parts.push("--id".to_string());
        parts.push(id.to_string());
    }
    if let Some(ref pk) = args.project_key {
        parts.push("--project-key".to_string());
        parts.push(pk.clone());
    }
    if let Some(ref s) = args.script {
        parts.push("--script".to_string());
        parts.push(s.clone());
    }
    if let Some(ref n) = args.name {
        parts.push("--name".to_string());
        parts.push(n.clone());
    }
    if args.persistent {
        parts.push("--persistent".to_string());
    }
    parts.join("\x00") // null-separated for safe splitting
}

#[derive(Debug, Clone)]
pub struct WatcherArgs {
    pub id: Option<u64>,
    pub project_key: Option<String>,
    pub script: Option<String>,
    pub name: Option<String>,
    pub persistent: bool,
}

// ─── Pipeline watcher ────────────────────────────────────

fn watch_pipeline(
    store: &Store,
    azdo: Option<&config::AzdoConfig>,
    args: &WatcherArgs,
    watcher_id: &str,
    interval: u64,
) -> Result<()> {
    let build_id = args.id.ok_or_else(|| anyhow::anyhow!("--id required for pipeline watcher"))?;
    let azdo = azdo.ok_or_else(|| anyhow::anyhow!("AzDo not configured"))?;

    loop {
        store.update_watcher_check(watcher_id).ok();

        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/build/builds/{}?api-version=7.1",
            azdo.organization, azdo.project, build_id
        );

        if let Ok(json) = curl_get_json(&url) {
            let status = json_str(&json, "status");
            let result = json_str(&json, "result");

            if status == "completed" {
                let (level, title) = match result.as_str() {
                    "succeeded" => ("success", format!("✓ Build #{} succeeded", build_id)),
                    "failed" => ("error", format!("✘ Build #{} failed", build_id)),
                    "canceled" => ("warn", format!("⊘ Build #{} canceled", build_id)),
                    _ => ("info", format!("Build #{} completed: {}", build_id, result)),
                };
                let link = format!(
                    "https://dev.azure.com/{}/{}/_build/results?buildId={}",
                    azdo.organization, azdo.project, build_id
                );
                notify(store, level, &title, None, "pipeline", Some(&link));
                break;
            }
        }

        thread::sleep(Duration::from_secs(interval));
    }
    Ok(())
}

// ─── PR merge watcher ────────────────────────────────────

fn watch_pr_merge(
    store: &Store,
    azdo: Option<&config::AzdoConfig>,
    args: &WatcherArgs,
    watcher_id: &str,
    interval: u64,
) -> Result<()> {
    let pr_id = args.id.ok_or_else(|| anyhow::anyhow!("--id required for pr-merge watcher"))?;
    let azdo = azdo.ok_or_else(|| anyhow::anyhow!("AzDo not configured"))?;

    loop {
        store.update_watcher_check(watcher_id).ok();

        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/pullrequests/{}?api-version=7.1",
            azdo.organization, azdo.project, pr_id
        );

        if let Ok(json) = curl_get_json(&url) {
            let status = json_str(&json, "status");

            match status.as_str() {
                "completed" => {
                    notify(
                        store,
                        "success",
                        &format!("✓ PR #{} merged", pr_id),
                        None,
                        "pr",
                        Some(&format!(
                            "https://dev.azure.com/{}/{}/_git/pullrequest/{}",
                            azdo.organization, azdo.project, pr_id
                        )),
                    );
                    break;
                }
                "abandoned" => {
                    notify(
                        store,
                        "warn",
                        &format!("⊘ PR #{} abandoned", pr_id),
                        None,
                        "pr",
                        Some(&format!(
                            "https://dev.azure.com/{}/{}/_git/pullrequest/{}",
                            azdo.organization, azdo.project, pr_id
                        )),
                    );
                    break;
                }
                _ => {} // still active
            }
        }

        thread::sleep(Duration::from_secs(interval));
    }
    Ok(())
}

// ─── PR comments watcher ────────────────────────────────

fn watch_pr_comments(
    store: &Store,
    azdo: Option<&config::AzdoConfig>,
    args: &WatcherArgs,
    watcher_id: &str,
    interval: u64,
) -> Result<()> {
    let pr_id = args.id.ok_or_else(|| anyhow::anyhow!("--id required for pr-comments watcher"))?;
    let azdo = azdo.ok_or_else(|| anyhow::anyhow!("AzDo not configured"))?;

    let mut last_count: Option<u64> = None;

    loop {
        store.update_watcher_check(watcher_id).ok();

        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version=7.1",
            azdo.organization, azdo.project, azdo.project, pr_id
        );

        if let Ok(json) = curl_get_json(&url) {
            let count = json_count(&json, "value");

            if let Some(prev) = last_count {
                if count > prev {
                    let new_comments = count - prev;
                    notify(
                        store,
                        "info",
                        &format!("💬 {} new comment thread(s) on PR #{}", new_comments, pr_id),
                        None,
                        "pr-review",
                        Some(&format!(
                            "https://dev.azure.com/{}/{}/_git/pullrequest/{}",
                            azdo.organization, azdo.project, pr_id
                        )),
                    );
                }
            }
            last_count = Some(count);
        }

        // Also check if PR is closed — stop watching
        let pr_url = format!(
            "https://dev.azure.com/{}/{}/_apis/git/pullrequests/{}?api-version=7.1",
            azdo.organization, azdo.project, pr_id
        );
        if let Ok(pr_json) = curl_get_json(&pr_url) {
            let status = json_str(&pr_json, "status");
            if status == "completed" || status == "abandoned" {
                break;
            }
        }

        thread::sleep(Duration::from_secs(interval));
    }
    Ok(())
}

// ─── SonarQube watcher ──────────────────────────────────

fn watch_sonarqube(
    store: &Store,
    cfg: &config::AppConfig,
    args: &WatcherArgs,
    watcher_id: &str,
    interval: u64,
) -> Result<()> {
    let project_key = args
        .project_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--project-key required for sonarqube watcher"))?;

    let sonar_url = std::env::var("SONARQUBE_URL")
        .unwrap_or_else(|_| "https://sonarqube.example.com".to_string());
    let sonar_token = std::env::var("SONAR_TOKEN").unwrap_or_default();

    let api_path = if let Some(pr_id) = args.id {
        format!(
            "/api/qualitygates/project_status?projectKey={}&pullRequest={}",
            project_key, pr_id
        )
    } else {
        format!("/api/qualitygates/project_status?projectKey={}", project_key)
    };

    loop {
        store.update_watcher_check(watcher_id).ok();

        let url = format!("{}{}", sonar_url, api_path);
        let output = Command::new("curl")
            .args(["-s", "-u", &format!("{}:", sonar_token), &url])
            .output();

        if let Ok(out) = output {
            let body = String::from_utf8_lossy(&out.stdout);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(status) = json
                    .get("projectStatus")
                    .and_then(|ps| ps.get("status"))
                    .and_then(|s| s.as_str())
                {
                    match status {
                        "OK" => {
                            notify(
                                store,
                                "success",
                                &format!("✓ SonarQube gate passed: {}", project_key),
                                None,
                                "sonarqube",
                                None,
                            );
                            break;
                        }
                        "ERROR" => {
                            notify(
                                store,
                                "error",
                                &format!("✘ SonarQube gate failed: {}", project_key),
                                None,
                                "sonarqube",
                                None,
                            );
                            break;
                        }
                        _ => {} // WARN, NONE — keep watching
                    }
                }
            }
        }

        thread::sleep(Duration::from_secs(interval));
    }
    Ok(())
}

// ─── Custom script watcher ──────────────────────────────

fn watch_custom(
    store: &Store,
    args: &WatcherArgs,
    watcher_id: &str,
    interval: u64,
) -> Result<()> {
    let script = args
        .script
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--script required for custom watcher"))?;

    // For persistent watchers: track last exit status to notify on transitions only
    let mut last_success: Option<bool> = None;

    loop {
        store.update_watcher_check(watcher_id).ok();

        let output = Command::new("bash")
            .args(["-c", script])
            .output();

        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim();

            if !first_line.is_empty() {
                store.update_watcher_output(watcher_id, first_line).ok();
            }

            let success = out.status.success();

            if args.persistent {
                // Persistent: notify on state transitions (OK↔FAIL), never break
                let transitioned = last_success.map(|prev| prev != success).unwrap_or(false);
                if transitioned {
                    if success {
                        let title = if first_line.is_empty() { "Service recovered ✓" } else { first_line };
                        notify(store, "success", title, None, "custom", None);
                    } else {
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let err_line = stderr.lines().next().unwrap_or("check failed");
                        let title = if first_line.is_empty() { err_line } else { first_line };
                        notify(store, "error", &format!("⚠ {}", title), None, "custom", None);
                    }
                }
                last_success = Some(success);
            } else {
                // Ephemeral: break on success (condition met)
                if success {
                    let title = if first_line.is_empty() { "Custom watcher triggered" } else { first_line };
                    let body = if stdout.lines().count() > 1 {
                        Some(stdout.lines().skip(1).collect::<Vec<_>>().join("\n"))
                    } else {
                        None
                    };
                    notify(store, "info", title, body.as_deref(), "custom", None);
                    break;
                }
            }
        }

        thread::sleep(Duration::from_secs(interval));
    }
    Ok(())
}

// ─── List active watchers ────────────────────────────────

pub fn list_watchers() -> Result<()> {
    let store = Store::open()?;
    let watchers = store.list_watchers()?;

    if watchers.is_empty() {
        println!("No active watchers.");
        return Ok(());
    }

    for w in &watchers {
        let alive = w
            .pid
            .map(|pid| is_process_alive(pid))
            .unwrap_or(false);
        let status = if alive {
            &w.status
        } else if w.status == "running" {
            "dead"
        } else {
            &w.status
        };
        let mode_icon = if w.persistent { "🔄" } else { "⚡" };
        let output_str = w.last_output.as_deref().unwrap_or("");
        if output_str.is_empty() {
            println!(
                "  {} {} [{}] {} (pid: {}, since: {})",
                mode_icon,
                w.id,
                status,
                w.watcher_type,
                w.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string()),
                &w.started_at[..16.min(w.started_at.len())],
            );
        } else {
            println!(
                "  {} {} [{}] {} — {} (pid: {}, since: {})",
                mode_icon,
                w.id,
                status,
                w.watcher_type,
                output_str,
                w.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string()),
                &w.started_at[..16.min(w.started_at.len())],
            );
        }
    }
    Ok(())
}

/// Stop a watcher by ID (kills the process)
pub fn stop_watcher(id: &str) -> Result<()> {
    let store = Store::open()?;
    let watchers = store.list_watchers()?;

    let watcher = watchers
        .iter()
        .find(|w| w.id == id || w.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("Watcher '{}' not found", id))?;

    if let Some(pid) = watcher.pid {
        if is_process_alive(pid) {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            eprintln!("Stopped watcher {} (pid {})", watcher.id, pid);
        }
    }
    store.update_watcher_status(&watcher.id, "stopped")?;
    Ok(())
}

/// Clean up dead watcher entries
pub fn cleanup_watchers() -> Result<()> {
    let store = Store::open()?;
    let watchers = store.list_watchers()?;
    let mut cleaned = 0;

    for w in &watchers {
        let alive = w.pid.map(|p| is_process_alive(p)).unwrap_or(false);
        if !alive && w.status == "running" {
            store.update_watcher_status(&w.id, "dead")?;
            cleaned += 1;
        }
    }

    if cleaned > 0 {
        eprintln!("Cleaned up {} dead watcher(s)", cleaned);
    }
    Ok(())
}

/// Resurrect persistent watchers that should be running but aren't.
/// Called at tmux startup to survive restarts.
pub fn resurrect_watchers() -> Result<()> {
    let store = Store::open()?;
    let watchers = store.list_watchers()?;
    let mut resurrected = 0;

    for w in &watchers {
        if !w.persistent {
            // Ephemeral dead watchers: clean up
            let alive = w.pid.map(|p| is_process_alive(p)).unwrap_or(false);
            if !alive && w.status == "running" {
                store.delete_watcher(&w.id).ok();
            }
            continue;
        }

        // Persistent watcher: check if it needs resurrection
        let alive = w.pid.map(|p| is_process_alive(p)).unwrap_or(false);
        if alive {
            continue; // already running
        }

        let args_str = match &w.restart_args {
            Some(a) if !a.is_empty() => a.clone(),
            _ => {
                eprintln!("  ⚠ Persistent watcher '{}' has no restart args, skipping", w.id);
                store.update_watcher_status(&w.id, "dead").ok();
                continue;
            }
        };

        // Delete old entry so the new process can re-register with same ID
        store.delete_watcher(&w.id)?;

        let args: Vec<&str> = args_str.split('\x00').collect();
        let exe = std::env::current_exe()?;
        match std::process::Command::new(&exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_child) => {
                resurrected += 1;
                eprintln!("  🔄 Resurrected watcher '{}'", w.id);
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to resurrect '{}': {}", w.id, e);
            }
        }
    }

    if resurrected > 0 {
        eprintln!("Resurrected {} persistent watcher(s)", resurrected);
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────

fn notify(
    store: &Store,
    level: &str,
    title: &str,
    body: Option<&str>,
    source: &str,
    link: Option<&str>,
) {
    store.add_notification(level, title, body, Some(source), link).ok();

    // Fire native notification and/or sound
    let cfg = config::AppConfig::load().ok();
    let native = cfg.as_ref().map(|c| c.notify.native).unwrap_or(false);
    let sound = cfg.as_ref().map(|c| c.notify.sound).unwrap_or(true);
    if native {
        crate::send_native_notification(title, body, sound);
    } else if sound {
        crate::play_notification_sound();
    }

    // Refresh tmux status bar
    let _ = Command::new("tmux")
        .args(["refresh-client", "-S"])
        .output();
}

fn curl_get_json(url: &str) -> Result<serde_json::Value> {
    let pat = std::env::var("AZURE_DEVOPS_PAT").unwrap_or_default();
    let output = Command::new("curl")
        .args([
            "-s",
            "-u",
            &format!(":{}", pat),
            url,
        ])
        .output()?;

    let body = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    Ok(json)
}

fn json_str(json: &serde_json::Value, key: &str) -> String {
    json.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn json_count(json: &serde_json::Value, key: &str) -> u64 {
    json.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u64)
        .unwrap_or(0)
}

pub fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
