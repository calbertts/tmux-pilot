use anyhow::Result;
use clap::{Parser, Subcommand};

mod azdo;
mod config;
mod copilot;
mod store;
mod tmux;
mod tui;
mod watcher;
mod wizard;

use store::Store;

#[derive(Parser)]
#[command(name = "pilot", about = "tmux session manager for AI coding agents")]
#[command(version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Open the feature selector (default when no subcommand)
    Open,
    /// Open the task selector for the current session's feature
    Task,
    /// Show the session dashboard
    Dash,
    /// List active sessions
    #[command(name = "ls")]
    List,
    /// Create a free session (no AzDo link)
    Free {
        /// Session name
        name: String,
    },
    /// Push a notification
    Notify {
        /// Notification title
        title: String,
        /// Notification body
        #[arg(short, long)]
        body: Option<String>,
        /// Level: info, warn, error, success
        #[arg(short, long, default_value = "info")]
        level: String,
        /// Source identifier (e.g. "pipeline", "pr-review")
        #[arg(short, long)]
        source: Option<String>,
        /// Link (URL or tmux target)
        #[arg(long)]
        link: Option<String>,
    },
    /// Show notifications or notification count
    Notifications {
        /// Show only the unread count
        #[arg(long)]
        count: bool,
        /// Output format for count: "plain" or "tmux"
        #[arg(long, default_value = "plain")]
        format: String,
        /// Mark all as read
        #[arg(long)]
        clear: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start a background watcher
    Watch {
        /// Watcher type: pipeline, pr-merge, pr-comments, sonarqube, custom
        #[arg(value_name = "TYPE")]
        watcher_type: String,
        /// ID (build ID, PR ID, etc.)
        #[arg(long)]
        id: Option<u64>,
        /// SonarQube project key
        #[arg(long)]
        project_key: Option<String>,
        /// Custom script to run
        #[arg(long)]
        script: Option<String>,
        /// Poll interval in seconds
        #[arg(long, default_value = "120")]
        interval: u64,
        /// Run in foreground (don't detach)
        #[arg(long)]
        foreground: bool,
    },
    /// List or manage active watchers
    Watchers {
        /// Stop a watcher by ID
        #[arg(long)]
        stop: Option<String>,
        /// Clean up dead watcher entries
        #[arg(long)]
        cleanup: bool,
        /// Open interactive TUI
        #[arg(long)]
        tui: bool,
    },
    /// Show or edit configuration
    Config,
    /// Run the setup wizard to configure AzDo connection
    Setup,
    /// Show detailed help with all features and keybindings
    #[command(name = "help-all")]
    HelpAll,
    /// Link a copilot session ID to a tmux window (internal use)
    #[command(name = "session-link", hide = true)]
    SessionLink {
        /// tmux session name
        session_name: String,
        /// tmux window name
        window_name: String,
        /// copilot session UUID
        copilot_session_id: String,
    },
    /// Restore copilot sessions after a tmux restart
    Restore,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("pilot=info".parse()?),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Short-circuit for help-all (no config needed)
    if matches!(cli.command, Some(Commands::HelpAll)) {
        return show_help_all();
    }

    // Short-circuit for session-link (internal, no config needed)
    if let Some(Commands::SessionLink { session_name, window_name, copilot_session_id }) = cli.command {
        return cmd_session_link(&session_name, &window_name, &copilot_session_id);
    }

    let mut cfg = config::AppConfig::load()?;

    // Auto-trigger setup if AzDo not configured and using a command that needs it
    let needs_azdo = matches!(
        cli.command.as_ref().unwrap_or(&Commands::Open),
        Commands::Open | Commands::Task
    );
    if needs_azdo && cfg.azdo.is_none() {
        eprintln!("⚠ AzDo not configured. Run `pilot setup` to connect to Azure DevOps.\n");
    }

    match cli.command.unwrap_or(Commands::Open) {
        Commands::Open => tui::run_feature_selector(&cfg).await,
        Commands::Task => tui::run_task_selector(&cfg).await,
        Commands::Dash => tui::run_dashboard(&cfg).await,
        Commands::List => list_sessions().await,
        Commands::Free { name } => create_free_session(&cfg, &name).await,
        Commands::Notify {
            title,
            body,
            level,
            source,
            link,
        } => cmd_notify(&title, body.as_deref(), &level, source.as_deref(), link.as_deref()),
        Commands::Notifications {
            count,
            format,
            clear,
            json,
        } => cmd_notifications(count, &format, clear, json),
        Commands::Watch {
            watcher_type,
            id,
            project_key,
            script,
            interval,
            foreground,
        } => cmd_watch(&watcher_type, id, project_key, script, interval, foreground),
        Commands::Watchers { stop, cleanup, tui: show_tui } => cmd_watchers(stop, cleanup, show_tui),
        Commands::Config => show_config(&cfg),
        Commands::Setup => wizard::run_wizard(&mut cfg).await.map(|_| ()),
        Commands::HelpAll => show_help_all(),
        Commands::SessionLink { .. } => unreachable!(), // handled above
        Commands::Restore => cmd_restore(&cfg),
    }
}

async fn list_sessions() -> Result<()> {
    let sessions = tmux::list_sessions()?;
    if sessions.is_empty() {
        println!("No active tmux sessions.");
        return Ok(());
    }
    for s in &sessions {
        println!(
            "  {} ({} windows){}",
            s.name,
            s.window_count,
            if s.attached { " *" } else { "" }
        );
    }
    Ok(())
}

async fn create_free_session(cfg: &config::AppConfig, name: &str) -> Result<()> {
    let store = store::Store::open()?;
    let session_name = name.to_string();
    if tmux::session_exists(&session_name)? {
        println!("Session '{}' already exists, switching...", session_name);
        tmux::switch_session(&session_name)?;
    } else {
        tmux::create_session(&session_name, None)?;

        store.save_session_mapping(&store::SessionMapping {
            session_name: session_name.clone(),
            work_item_id: None,
            work_item_title: None,
            work_item_type: Some("Free".to_string()),
            template: None,
            created_at: String::new(),
        })?;

        println!("Created session '{}'", session_name);

        if cfg.copilot.auto_launch {
            copilot::launch_in_current_pane(cfg, None)?;
        }
    }
    Ok(())
}

fn show_config(cfg: &config::AppConfig) -> Result<()> {
    println!("Configuration:");
    println!("  Copilot binary: {}", cfg.copilot.bin);
    println!("  Yolo mode: {}", cfg.copilot.yolo);
    if let Some(ref agent) = cfg.copilot.default_agent {
        println!("  Default agent: {}", agent);
    }
    if let Some(ref azdo) = cfg.azdo {
        println!("  AzDo org: {}", azdo.organization);
        println!("  AzDo project: {}", azdo.project);
    } else {
        println!("  AzDo: not configured");
    }
    println!("\n  Config file: {}", config::config_path().display());
    Ok(())
}

fn cmd_notify(
    title: &str,
    body: Option<&str>,
    level: &str,
    source: Option<&str>,
    link: Option<&str>,
) -> Result<()> {
    let store = store::Store::open()?;
    let id = store.add_notification(level, title, body, source, link)?;

    // Fire native OS notification if configured
    let cfg = config::AppConfig::load()?;
    if cfg.notify.native {
        send_native_notification(title, body);
    }

    // Refresh tmux status bar to show updated count
    let _ = std::process::Command::new("tmux")
        .args(["refresh-client", "-S"])
        .output();

    eprintln!("🔔 Notification #{} created ({})", id, level);
    Ok(())
}

pub fn send_native_notification(title: &str, body: Option<&str>) {
    let body_text = body.unwrap_or("");

    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"pilot\" subtitle \"{}\"",
                    body_text.replace('"', "\\\""),
                    title.replace('"', "\\\""),
                ),
            ])
            .output();
    } else if cfg!(target_os = "windows") {
        // PowerShell toast notification (Windows 10+)
        let ps_script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $text = $template.GetElementsByTagName('text'); \
             $text.Item(0).AppendChild($template.CreateTextNode('pilot: {}')) > $null; \
             $text.Item(1).AppendChild($template.CreateTextNode('{}')) > $null; \
             $toast = [Windows.UI.Notifications.ToastNotification]::new($template); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('pilot').Show($toast)",
            title.replace('\'', "''"),
            body_text.replace('\'', "''"),
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_script])
            .output();
    } else {
        // Linux: try notify-send (common on most distros)
        let _ = std::process::Command::new("notify-send")
            .args([
                "--app-name=pilot",
                &format!("pilot: {}", title),
                body_text,
            ])
            .output();
    }
}

fn cmd_notifications(count: bool, format: &str, clear: bool, json: bool) -> Result<()> {
    let store = store::Store::open()?;

    // Auto-cleanup old notifications
    store.cleanup_old_notifications(7)?;

    if clear {
        let n = store.mark_all_read()?;
        eprintln!("✓ Marked {} notifications as read", n);
        // Refresh tmux status bar
        let _ = std::process::Command::new("tmux")
            .args(["refresh-client", "-S"])
            .output();
        return Ok(());
    }

    if count {
        let n = store.unread_count()?;
        match format {
            "tmux" => {
                if n > 0 {
                    print!("#[fg=colour208,bold]🔔 {} #[default]", n);
                }
                // Empty output when 0 — keeps status bar clean
            }
            _ => println!("{}", n),
        }
        return Ok(());
    }

    if json {
        let notifications = store.list_notifications(100)?;
        let unread: Vec<_> = notifications.into_iter().filter(|n| !n.read).collect();
        // Simple JSON array
        print!("[");
        for (i, n) in unread.iter().enumerate() {
            if i > 0 {
                print!(",");
            }
            print!(
                "{{\"id\":{},\"level\":\"{}\",\"title\":\"{}\",\"source\":{},\"created_at\":\"{}\"}}",
                n.id,
                n.level,
                n.title.replace('"', "\\\""),
                n.source
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "null".to_string()),
                n.created_at,
            );
        }
        println!("]");
        return Ok(());
    }

    // Default: open notification center TUI
    tui::run_notifications_sync(&store)
}

fn cmd_watch(
    watcher_type: &str,
    id: Option<u64>,
    project_key: Option<String>,
    script: Option<String>,
    interval: u64,
    foreground: bool,
) -> Result<()> {
    let args = watcher::WatcherArgs {
        id,
        project_key,
        script,
    };

    if foreground {
        eprintln!("👁 Starting {} watcher (foreground, interval: {}s)", watcher_type, interval);
        watcher::run_watcher(watcher_type, &args, interval)
    } else {
        // Re-launch self as a detached background process with --foreground
        let exe = std::env::current_exe()?;
        let mut cmd_args = vec![
            "watch".to_string(),
            watcher_type.to_string(),
            "--interval".to_string(),
            interval.to_string(),
            "--foreground".to_string(),
        ];
        if let Some(id_val) = id {
            cmd_args.push("--id".to_string());
            cmd_args.push(id_val.to_string());
        }
        if let Some(ref pk) = args.project_key {
            cmd_args.push("--project-key".to_string());
            cmd_args.push(pk.clone());
        }
        if let Some(ref s) = args.script {
            cmd_args.push("--script".to_string());
            cmd_args.push(s.clone());
        }

        let child = std::process::Command::new(&exe)
            .args(&cmd_args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        eprintln!(
            "👁 Watcher started in background (pid: {}, type: {}, interval: {}s)",
            child.id(),
            watcher_type,
            interval
        );
        eprintln!("   Use `pilot watchers` to list, `pilot watchers --stop <id>` to stop");
        Ok(())
    }
}

fn cmd_watchers(stop: Option<String>, cleanup: bool, show_tui: bool) -> Result<()> {
    if let Some(id) = stop {
        return watcher::stop_watcher(&id);
    }
    if cleanup {
        return watcher::cleanup_watchers();
    }
    if show_tui {
        let store = Store::open()?;
        return tui::run_watchers_sync(&store);
    }
    watcher::list_watchers()
}

fn show_help_all() -> Result<()> {
    let help = r#"
  tmux-pilot — tmux session manager for AI coding agents

  ━━━ Quick Start ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    1. pilot setup            Interactive wizard (PAT → org → project → team)
    2. Add to ~/.tmux.conf:   run-shell /path/to/tmux-pilot/pilot.tmux
    3. tmux source ~/.tmux.conf
    4. prefix + F              Open feature selector

  ━━━ CLI Commands ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    pilot                     Feature selector (default)
    pilot open                Feature selector
    pilot task                Task selector (current session's feature)
    pilot dash                Session dashboard
    pilot ls                  List active tmux sessions
    pilot free "Name"         Create a free session (no AzDo link)
    pilot config              Show current configuration
    pilot setup               Interactive setup wizard
    pilot restore             Restore copilot sessions after tmux restart

  ━━━ Notifications ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    pilot notify "title"      Push a notification
      -b "body"                 Optional body text
      -l warn|error|success     Level (default: info)
      -s "source"               Source tag (e.g., "pipeline")
      --link "url"              Clickable link
    pilot notifications       Open notification center TUI
      --count                   Show unread count
      --count --format tmux     For tmux status bar
      --clear                   Mark all as read
      --json                    Output as JSON

  ━━━ Watchers ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    pilot watch pipeline --id 12345     Watch a build
    pilot watch pr-merge --id 678       Watch PR for merge
    pilot watch pr-comments --id 678    Watch PR for new comments
    pilot watch sonarqube --project-key KEY  Watch quality gate
    pilot watch custom --script "cmd"   Run custom check
      --interval 120              Poll interval in seconds
    pilot watchers                List active watchers
    pilot watchers --tui          Interactive watcher manager
    pilot watchers --stop ID      Stop a watcher
    pilot watchers --cleanup      Remove dead entries

  ━━━ tmux Keybindings ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    prefix + F    Feature selector
    prefix + T    Task selector
    prefix + D    Session dashboard
    prefix + N    Notification center
    prefix + W    Watcher manager

    Customize via tmux options:
      set -g @pilot-feature-key "F"
      set -g @pilot-task-key "T"
      set -g @pilot-dash-key "D"
      set -g @pilot-notify-key "N"
      set -g @pilot-watcher-key "W"

  ━━━ TUI Navigation ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    j/k  ↑/↓       Navigate
    Enter           Select / open / attach
    o               Open detail (task view) / view tasks (feature view)
    Ctrl+O          Go back to previous view
    Ctrl+N          New session (feature) / new copilot window (task)
    Ctrl+T          New terminal window (task view)
    d               Kill session (dashboard)
    gg              Jump to top
    G (Shift+G)     Jump to bottom
    q / Esc         Quit
    Type            Fuzzy filter
    Backspace       Clear filter
    Mouse scroll    Navigate

  ━━━ State Badges ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    ○  New (blue)        ●  Active (green)
    ◉  Resolved (aqua)   ✔  Closed (gray)
    ⊕  Not yet started locally (AzDo only)

  ━━━ Work Item Icons ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    🏗  Feature    📖  User Story    🐛  Bug    ✅  Task    📁  Free

  ━━━ Environment Variables ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    AZURE_DEVOPS_PAT      AzDo personal access token (required)
    PILOT_AZDO_PAT        Alternative PAT variable
    PILOT_AZDO_ORG        AzDo organization (overrides config)
    PILOT_AZDO_PROJECT    AzDo project (overrides config)
    PILOT_AZDO_TEAM       Team name (overrides config)
    PILOT_AZDO_AREA       Area path filter (overrides config)
    PILOT_CODE_PATH       Code directory (auto-adds --add-dir to copilot)

  ━━━ Config File ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    macOS:  ~/Library/Application Support/pilot/config.toml
    Linux:  ~/.config/pilot/config.toml

    Run `pilot setup` to create interactively, or `pilot config` to view.
"#;
    print!("{}", help);
    Ok(())
}

fn cmd_session_link(session_name: &str, window_name: &str, copilot_session_id: &str) -> Result<()> {
    let store = Store::open()?;
    store.upsert_copilot_session_id(session_name, window_name, copilot_session_id)?;
    Ok(())
}

fn cmd_restore(cfg: &config::AppConfig) -> Result<()> {
    let store = Store::open()?;
    let mappings = store.get_all_window_mappings_with_sessions()?;

    if mappings.is_empty() {
        println!("No copilot sessions to restore.");
        return Ok(());
    }

    let sessions = tmux::list_sessions()?;
    let session_names: std::collections::HashSet<String> =
        sessions.iter().map(|s| s.name.clone()).collect();

    let session_state_dir = dirs::home_dir()
        .map(|h| h.join(".copilot/session-state"));

    let mut restored = 0;
    for mapping in &mappings {
        if !session_names.contains(&mapping.session_name) {
            continue;
        }

        let windows = match tmux::list_windows(&mapping.session_name) {
            Ok(w) => w,
            Err(_) => continue,
        };
        if !windows.iter().any(|w| w.name == mapping.window_name) {
            continue;
        }

        let target = format!("{}:{}", mapping.session_name, mapping.window_name);
        if is_copilot_running(&target) {
            continue;
        }

        let session_id = match &mapping.copilot_session_id {
            Some(id) => id,
            None => continue,
        };

        // Verify the copilot session state dir still exists
        if let Some(ref base) = session_state_dir {
            if !base.join(session_id).exists() {
                continue;
            }
        }

        if copilot::resume_in_target(cfg, &target, session_id).is_ok() {
            restored += 1;
        }
    }

    if restored > 0 {
        println!("✓ Restored {} copilot session(s)", restored);
    } else {
        println!("No copilot sessions needed restoring.");
    }
    Ok(())
}

/// Check if copilot (or node, which copilot runs as) is active in a pane
fn is_copilot_running(target: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["display-message", "-t", target, "-p", "#{pane_current_command}"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|cmd| {
            let cmd = cmd.trim();
            cmd == "node" || cmd.contains("copilot")
        })
        .unwrap_or(false)
}
