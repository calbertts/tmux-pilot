# tmux-pilot

A tmux plugin for managing AI coding sessions with Azure DevOps integration. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui).

Organize tmux **sessions** around AzDo **features** and **windows** around **user stories/bugs/tasks**. Auto-launch [GitHub Copilot CLI](https://githubnext.com/projects/copilot-cli) with work item context injection.

<p align="center"><img src="docs/screenshots/demo.gif?v=3" width="800" /></p>

> Feature selector → Task selector → Dashboard → Help reference. Run `pilot --demo` to try it yourself.

## Getting Started

### Prerequisites

- **tmux 3.3+** (needs `display-popup` support)
- **macOS** (arm64/x86_64) or **Linux** (x86_64/aarch64)
- **GitHub Copilot CLI** installed and authenticated (`copilot` in PATH)
- An **Azure DevOps** organization with Features/User Stories/Bugs

### 1. Install

**With [TPM](https://github.com/tmux-plugins/tpm)** (recommended):

```tmux
# Add to ~/.tmux.conf
set -g @plugin 'calbertts/tmux-pilot'
```

Then `prefix + I` to install. The binary is auto-downloaded from GitHub Releases.

**Manual:**

```bash
git clone https://github.com/calbertts/tmux-pilot.git ~/.tmux/plugins/tmux-pilot
echo 'run-shell ~/.tmux/plugins/tmux-pilot/pilot.tmux' >> ~/.tmux.conf
tmux source ~/.tmux.conf
```

> To build from source: `cd ~/.tmux/plugins/tmux-pilot && cargo build --release`

### 2. Configure AzDo connection

```bash
# Set your PAT (add to your shell profile so it persists across tmux restarts)
export AZURE_DEVOPS_PAT="your-pat-here"

# Run the setup wizard
pilot setup
```

The wizard walks you through: **organization → project → team → area path → iteration filters**.

Config is saved to `~/.config/pilot/config.toml` (Linux) or `~/Library/Application Support/pilot/config.toml` (macOS).

### 3. Open pilot

```
prefix + F
```

That's it. You'll see your AzDo features grouped by state. Navigate with `j/k`, press `Enter` to create a tmux session for a feature, and `o` to drill into its children.

### First session walkthrough

1. `prefix + F` — opens the feature selector
2. Navigate to a feature → `Enter` — creates a tmux session named after the feature
3. `prefix + T` — shows the tasks/stories/bugs under that feature
4. Select a task → `Enter` — creates a tmux window and auto-launches copilot with the work item context
5. Start coding — copilot already knows what you're working on

## Features

| Feature | Key | Description |
|---------|-----|-------------|
| **Feature selector** | `prefix+F` | Sessions grouped by AzDo state: Active, New, AzDo-only, Free |
| **Task selector** | `prefix+T` | Windows grouped by type: 🐛 Bugs, 📖 User Stories, ✅ Tasks, 💻 Free |
| **Dashboard** | `prefix+D` | Overview of all sessions with window previews |
| **Notification center** | `prefix+N` | 🔔 status bar badge, level icons, source tags |
| **Watcher manager** | `prefix+W` | Background monitors grouped: 🔄 Persistent / ⚡ Ephemeral |
| **Detail view** | `d` | Read description + acceptance criteria inline |
| **Hierarchy navigation** | `o` / `⌫` | Drill into children (Feature → Story → Bug/Task) and back |
| **Copilot auto-launch** | — | Launches copilot with work item context (title, description, acceptance criteria) |
| **Fuzzy filter** | type | Filter any list by typing |
| **Native notifications** | — | macOS / Linux desktop notifications with sound |
| **Session persistence** | — | Copilot sessions survive tmux restarts (`scan` + `restore`) |
| **Watcher persistence** | — | Persistent watchers auto-resurrect after restarts |

## TUI Navigation

| Key | Action |
|-----|--------|
| `j/k` or `↑/↓` | Navigate |
| `Enter` | Select / open / attach |
| `o` | Drill into children (feature → tasks → sub-items) |
| `d` | View work item detail |
| `Backspace` / `Ctrl+O` | Go back (hierarchy or filter) |
| `Ctrl+N` | New session (features) / new copilot window (tasks) |
| `Ctrl+T` | New terminal window (tasks) |
| `gg` / `G` | Jump to top / bottom |
| type | Fuzzy filter |
| `q` / `Esc` | Quit |

## CLI Reference

```bash
# Views
pilot              # Feature selector (default)
pilot task         # Task selector
pilot dash         # Dashboard
pilot ls           # List sessions
pilot free "Name"  # Create a free session (no AzDo link)

# Notifications
pilot notify "Build failed" -l error -s pipeline
pilot notifications           # Open notification center
pilot notifications --count   # Unread count (used in tmux status bar)

# Watchers — ephemeral (default) auto-delete on completion
pilot watch pipeline --name pipe-123 --id 12345
pilot watch pr-merge --name pr-678 --id 678
pilot watch custom --name my-check --script "check.sh" --interval 30

# Watchers — persistent: survive restarts, notify on state transitions only
pilot watch custom --name api-health --persistent --script "curl -sf https://api/health" --interval 60

# Watcher management
pilot watchers                    # List all (🔄 persistent, ⚡ ephemeral)
pilot watchers --tui              # Interactive manager
pilot watchers --stop my-check    # Stop by name

# Config
pilot setup        # Setup wizard
pilot config       # Show current config
pilot help-all     # Full CLI reference
```

## Configuration

Minimal config (created by `pilot setup`):

```toml
[copilot]
bin = "copilot"
yolo = true
auto_launch = true

[azdo]
organization = "my-org"
project = "My-Project"
team = "my-team"

[azdo.filters]
iteration = "current"
states = ["New", "Active", "Resolved"]
area_paths = ["My-Project\\My-Team"]

[notify]
native = true
sound = true
ttl_days = 7
```

See [`config.example.toml`](config.example.toml) for all options including prompt templates, keybinding overrides, and extra copilot flags.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `AZURE_DEVOPS_PAT` | AzDo personal access token **(required)** |
| `PILOT_AZDO_ORG` | Override organization from config |
| `PILOT_AZDO_PROJECT` | Override project from config |
| `PILOT_AZDO_TEAM` | Override team from config |
| `PILOT_AZDO_AREA` | Override area path filter |
| `PILOT_CODE_PATH` | Auto-add `--add-dir` to copilot |

> **Tip:** Export `AZURE_DEVOPS_PAT` in your shell profile (`~/.zshrc`, `~/.bashrc`) so it's available in every tmux session.

### Keybinding Customization

Override default keys in `~/.tmux.conf`:

```tmux
set -g @pilot-feature-key "F"
set -g @pilot-task-key "T"
set -g @pilot-dash-key "D"
set -g @pilot-notify-key "N"
set -g @pilot-watcher-key "W"
```

## Bundled Copilot Skills

tmux-pilot ships copilot-cli skills that are **automatically installed** when the plugin loads (symlinked to `~/.copilot/skills/`).

### pilot-watcher

Enables copilot to launch and manage watchers directly from conversation:

```
> Start a watcher for pipeline build 12345
> Show me active watchers
> Stop the PR merge watcher
```

No configuration needed — available in every copilot session automatically.

## Architecture

```
pilot (~4MB Rust binary)
├── TUI (ratatui + crossterm)
│   ├── Feature Selector — sessions grouped by AzDo state, fuzzy filter
│   ├── Task Selector — windows grouped by type, hierarchy navigation
│   ├── Dashboard — session overview with window previews
│   ├── Notification Center — level icons, source tags, read/unread
│   └── Watcher Manager — persistent/ephemeral groups, stop/restart/delete
├── tmux Controller — session/window CRUD via tmux CLI
├── Copilot Launcher — context injection from work items
├── AzDo Client — REST API via curl subprocess (Zscaler-compatible)
├── Notification System — SQLite → tmux status bar → native OS
├── Watcher Framework — pipeline, PR, SonarQube, custom monitors
├── SQLite Store — sessions, notifications, watchers, AzDo cache
├── Bundled Skills — auto-installed to ~/.copilot/skills/
└── Config — TOML + env vars + interactive setup wizard
```

## License

MIT
