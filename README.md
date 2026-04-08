# tmux-pilot

A tmux plugin for managing AI coding sessions with Azure DevOps integration. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui).

Organize tmux **sessions** around AzDo **features** and **windows** around **user stories/bugs/tasks**. Auto-launch `copilot` CLI with work item context injection.

## Demo

<p align="center"><img src="docs/screenshots/demo.gif?v=3" width="800" /></p>

> Feature selector → Task selector → Dashboard → Help reference. Run `pilot --demo` to try it yourself.

## Features

- **Feature selector** (`prefix+F`) — grouped view: Active, AzDo-only, Free sessions
- **Task selector** (`prefix+T`) — grouped by type: Bugs 🐛, User Stories 📖, Tasks ✅, Free 💻
- **Dashboard** (`prefix+D`) — overview of all sessions with window previews
- **Notification center** (`prefix+N`) — 🔔 in status bar, level icons, source tags
- **Watcher manager** (`prefix+W`) — background monitors for pipelines, PRs, SonarQube, custom scripts with live progress
- **Detail view** — press `o` on any work item to read description + acceptance criteria
- **Copilot integration** — auto-launch copilot with work item context injection
- **AzDo integration** — fetch features/stories/bugs via REST API (curl-based, Zscaler-compatible)
- **Fuzzy search** — type to filter in any view
- **Native notifications** — macOS, Windows, Linux desktop notifications with sound
- **Notification sound** — configurable ping sound on every notification (on by default)
- **Session persistence** — copilot sessions survive tmux restarts via `pilot scan` + `pilot restore`
- **SQLite persistence** — session mappings, notifications, watchers, AzDo cache

## Installation

### Option A: TPM (recommended)

Add to `~/.tmux.conf`:

```tmux
set -g @plugin 'calbertts/tmux-pilot'
```

Run `prefix + I` to install. The binary is auto-downloaded from GitHub Releases.

### Option B: Manual

```bash
git clone https://github.com/calbertts/tmux-pilot.git ~/.tmux/plugins/tmux-pilot
```

Add to `~/.tmux.conf`:

```tmux
run-shell ~/.tmux/plugins/tmux-pilot/pilot.tmux
```

Reload: `tmux source ~/.tmux.conf`

The binary auto-downloads on first load. To build from source instead:

```bash
cd ~/.tmux/plugins/tmux-pilot
cargo build --release
```

### Setup

```bash
pilot setup   # Interactive wizard: PAT → org → project → team → area path
```

## Usage

Run `pilot help-all` for the complete reference, or see below:

### CLI

```bash
pilot              # Feature selector (default)
pilot task         # Task selector
pilot dash         # Dashboard
pilot ls           # List sessions
pilot free "Name"  # Free session
pilot setup        # Setup wizard
pilot config       # Show config
pilot help-all     # Full reference
```

### Notifications & Watchers

```bash
pilot notify "Build failed" -l error -s pipeline
pilot watch pipeline --name pipe-pr123 --id 12345
pilot watch custom --name my-download --script "check.sh" --interval 30
pilot watchers                  # Shows progress for custom watchers
pilot watchers --stop my-download
pilot watchers --tui
```

### tmux Keybindings

| Key | Action |
|-----|--------|
| `prefix + F` | Feature selector |
| `prefix + T` | Task selector |
| `prefix + D` | Session dashboard |
| `prefix + N` | Notification center |
| `prefix + W` | Watcher manager |

### TUI Navigation

| Key | Action |
|-----|--------|
| `j/k` `↑/↓` | Navigate |
| `Enter` | Select / open / attach |
| `o` | View detail / tasks |
| `Ctrl+O` | Go back |
| `Ctrl+N` | New session / copilot window |
| `gg` / `G` | Jump to top / bottom |
| Type | Fuzzy filter |
| `q` / `Esc` | Quit |

## Configuration

Config file: `~/.config/pilot/config.toml` (Linux) or `~/Library/Application Support/pilot/config.toml` (macOS).

Run `pilot setup` for interactive configuration, or create manually:

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

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `AZURE_DEVOPS_PAT` | AzDo personal access token (required) |
| `PILOT_AZDO_ORG` | Override organization from config |
| `PILOT_AZDO_PROJECT` | Override project from config |
| `PILOT_AZDO_TEAM` | Override team from config |
| `PILOT_AZDO_AREA` | Override area path filter |
| `PILOT_CODE_PATH` | Auto-add `--add-dir` to copilot |

## Bundled Copilot Skills

tmux-pilot ships with copilot-cli skills that are **automatically installed** when the plugin loads.

On first load (or after updates), `pilot.tmux` symlinks the bundled skills into `~/.copilot/skills/` so copilot-cli discovers them without manual setup.

### pilot-watcher

Enables copilot to launch and manage watchers (pipeline monitors, PR trackers, SonarQube checks) directly from the conversation:

```
> Start a watcher for pipeline build 12345
> Show me active watchers
> Stop the PR merge watcher
```

The skill is available in every copilot session automatically — no configuration needed.

> **Note:** If you already have a `~/.copilot/skills/pilot-watcher` from a different source, the installer won't overwrite it. Remove it manually to use the bundled version.

## Architecture

```
pilot (~4MB binary)
├── TUI (ratatui + crossterm)
│   ├── Feature Selector — grouped, fuzzy, state badges
│   ├── Task Selector — grouped by type, detail view
│   ├── Dashboard — session overview
│   ├── Notification Center — level icons, source tags
│   └── Watcher Manager — status, stop, cleanup
├── tmux Controller — session/window CRUD
├── Copilot Launcher — context injection from work items
├── AzDo Client — REST via curl (Zscaler-compatible)
├── Notification System — SQLite → status bar → native OS
├── Watcher Framework — pipeline, PR, SonarQube, custom monitors
├── SQLite Store — sessions, notifications, watchers, AzDo cache
├── Bundled Skills — auto-installed to ~/.copilot/skills/
└── Config — TOML + env var enrichment + setup wizard
```

## License

MIT
