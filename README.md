# tcs — tmux copilot sessions

A tmux session manager for AI coding agents with Azure DevOps integration. Built in Rust with [ratatui](https://github.com/ratatui-org/ratatui).

Organize tmux **sessions** around AzDo **features** and **windows** around **user stories/bugs/tasks**. Auto-launch `copilot` CLI with work item context injection.

## Features

- **Feature selector** (`prefix+F`) — grouped view: Active (linked to AzDo), AzDo-only (not yet started), Free sessions
- **Task selector** (`prefix+T`) — grouped by type: Bugs 🐛, User Stories 📖, Tasks ✅, Free 💻. Clear new/existing differentiation
- **Dashboard** (`prefix+D`) — overview of all sessions with window previews
- **View toggle** — press `o` to switch between Feature↔Task↔Dashboard views
- **Copilot integration** — auto-launch `copilot --yolo -i "<context>"` with work item metadata
- **AzDo integration** — fetch features/stories/bugs via REST API (curl-based, Zscaler-compatible)
- **Fuzzy search** — type to filter in any view
- **Async loading** — local data instant, AzDo fetched in background with spinner
- **SQLite persistence** — session↔feature and window↔work-item mappings survive restarts
- **Gruvbox Dark theme** — matches your tmux config

## Installation

### Build from source

```bash
cd ~/code/siba/tmux-copilot-sessions
HTTPS_PROXY=http://127.0.0.1:18080 cargo build --release
cp target/release/tcs /opt/homebrew/bin/tcs
```

> **Note**: The `HTTPS_PROXY` is needed if behind Zscaler (corporate proxy). Without it, `cargo` can't download crates.

### tmux plugin setup

Add to `~/.tmux.conf`:

```tmux
run-shell ~/code/siba/tmux-copilot-sessions/tcs.tmux
```

Then reload: `tmux source ~/.tmux.conf`

The plugin auto-forwards `AZURE_DEVOPS_PAT` and `SIBA_*` env vars into the tmux server environment and binds the keybindings.

## Usage

### CLI

```bash
tcs              # Feature selector (default)
tcs task         # Task selector (current session)
tcs dash         # Dashboard
tcs ls           # List sessions
tcs free "Name"  # Create a free session
tcs config       # Show config
tcs setup        # Interactive setup wizard
```

### Keybindings (via tmux)

| Key | Action |
|-----|--------|
| `prefix + F` | Feature selector |
| `prefix + T` | Task selector |
| `prefix + D` | Session dashboard |

### TUI Navigation

| Key | Action |
|-----|--------|
| `j/k` or `↑/↓` | Navigate |
| `Enter` | Select / open / attach |
| `o` | Toggle view (Feature↔Task↔Dashboard) |
| `Ctrl+o` | Toggle view (from any view) |
| `n` | New free session (feature selector) |
| `c` | New copilot window (task selector) |
| `t` | New terminal window (task selector) |
| `d` | Kill session (dashboard) |
| `q` / `Esc` | Quit |
| Type anything | Fuzzy filter |
| `Backspace` | Clear filter |

### Visual guide

**Feature Selector** groups:
- `─── Active ───` — sessions linked to AzDo features (green, with window count)
- `─── AzDo ───` — features without a local session yet (gray + `⊕ new`)
- `─── Free ───` — sessions not linked to AzDo

**Task Selector** groups:
- `─── Bugs ───` — 🐛 yellow (existing) or gray (new)
- `─── User Stories ───` — 📖 blue (existing) or gray (new)
- `─── Tasks ───` — ✅ aqua (existing) or gray (new)
- `─── Free ───` — 💻 unlinked windows

## Configuration

Config lives at `~/Library/Application Support/tcs/config.toml` (macOS) or `~/.config/tcs/config.toml` (Linux).

Copy the example:

```bash
cp config.example.toml "$(dirs config)/tcs/config.toml"
```

Or run `tcs setup` for an interactive wizard.

### Key settings

```toml
[copilot]
bin = "copilot"
yolo = true
auto_launch = true
default_agent = "-nn-bank-siba-ai-agents:siba-developer-agent"
extra_flags = ["--add-dir", "~/code/siba"]

[azdo]
organization = "nn-bank"
project = "SIBA-Transformation-DFJ"
team = "nnb-siba-generic-team"
# PAT from env: AZURE_DEVOPS_PAT

[azdo.filters]
area_path = "SIBA-Transformation-DFJ\\nnb-siba-generic-team"
states = ["New", "Active", "Resolved"]
```

### Environment variables

| Variable | Purpose |
|----------|---------|
| `AZURE_DEVOPS_PAT` | AzDo personal access token |
| `SIBA_PROJECT_BACKLOG` | AzDo project name |
| `SIBA_AREA_PATH` | Team area path |
| `SIBA_TEAM` | Team name |
| `SIBA_ORG` | AzDo organization |

These are auto-detected if set. The `tcs.tmux` plugin forwards them into the tmux server.

## Architecture

```
tcs (3.8MB binary)
├── TUI (ratatui + crossterm)
│   ├── Feature Selector — grouped, fuzzy, visual_map navigation
│   ├── Task Selector — grouped by work item type
│   └── Dashboard — session overview
├── tmux Controller — session/window CRUD via CLI
├── Copilot Launcher — --yolo, --agent, -i context injection
├── AzDo Client — REST via curl subprocess (bypasses Zscaler)
├── SQLite Store — session/window mappings + AzDo cache (15min TTL)
└── Config — TOML + env var enrichment + setup wizard
```

### Why curl instead of reqwest?

Zscaler corporate proxy intercepts TLS at the process level. All Rust HTTP clients (reqwest with rustls-tls or native-tls) fail. The `curl` binary uses macOS SecureTransport which Zscaler trusts, so all AzDo API calls go through `curl` subprocess.

## Tech Stack

- **Rust 1.94+** (Homebrew)
- **ratatui 0.29** + crossterm 0.28 — TUI
- **clap 4** — CLI
- **tokio 1** — async runtime
- **rusqlite 0.32** (bundled) — SQLite
- **nucleo-matcher 0.3** — fuzzy matching
- **serde + toml** — config

## License

MIT
