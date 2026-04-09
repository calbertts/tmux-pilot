---
name: pilot-watcher
description: Launch background watchers via pilot to monitor pipelines, PRs, SonarQube gates, and custom events with notifications.
---

# Pilot Watcher Skill

Launch background watchers that monitor events and push notifications via `pilot notify`. Notifications appear in the tmux status bar (🔔) and optionally as native OS notifications.

## Watcher Modes

### ⚡ Ephemeral (default)
One-shot watchers that **auto-delete** when their condition is met. Use for:
- Downloads completing
- Pipelines finishing
- PRs being merged
- Any "wait for X then tell me" scenario

### 🔄 Persistent (`--persistent`)
Long-running service monitors that **survive restarts** and keep polling. Use for:
- API health checks
- Disk space monitoring
- Service uptime
- Any "keep watching forever" scenario

Persistent watchers:
- **Survive tmux/system restarts** — automatically resurrected on startup
- Notify only on **state transitions** (OK→FAIL, FAIL→OK) to avoid spam
- Stay in the watcher list even after stopping — can be **restarted** with `R` in TUI
- Must be explicitly deleted (`d` in TUI or `pilot watchers --stop`)

## When to Use

Activate this skill when the user asks to:
- Watch/monitor a pipeline, build, or CI run → **ephemeral**
- Get notified when a PR is merged, abandoned, or has new comments → **ephemeral**
- Monitor SonarQube quality gates → **ephemeral**
- Set up any recurring background check → **persistent if service, ephemeral if one-time**
- Track a download or long task → **ephemeral**
- Monitor a service or health endpoint → **persistent**

## Naming Convention (REQUIRED)

**Always use `--name`** to give every watcher a human-readable identifier. The name should be short, descriptive, and use kebab-case:

| Context | Name example | Mode |
|---------|-------------|------|
| Pipeline for PR #567 | `--name pipeline-pr567` | ephemeral |
| PR merge watch #890 | `--name pr-merge-890` | ephemeral |
| PR comments watch | `--name pr-comments-890` | ephemeral |
| SonarQube for a service | `--name sonar-auth-service` | ephemeral |
| Custom: file download | `--name gemma4-download` | ephemeral |
| Custom: API health check | `--name api-health --persistent` | persistent |
| Custom: disk space monitor | `--name disk-usage --persistent` | persistent |
| Custom: service uptime | `--name svc-payments --persistent` | persistent |

**Pattern**: `<type-or-purpose>-<target-identifier>`

Without `--name`, watchers get opaque IDs like `custom-81271` which are impossible to identify in `pilot watchers` output.

## Custom Script Progress Output

For custom watchers that track long-running tasks (downloads, migrations, etc.), the script should **always print a status line to stdout** — even when exiting non-zero (condition not met). The first line of stdout is captured as `last_output` and shown in `pilot watchers`.

**Example script pattern for progress tracking**:
```bash
# Exit 0 = done (triggers notification), exit 1 = still running (keeps polling)
# First line of stdout = progress shown in 'pilot watchers'
FILE="/path/to/download.bin"
EXPECTED=5220000000
if [ -f "$FILE" ] && [ $(stat -f%z "$FILE") -ge $EXPECTED ]; then
    echo "✅ Download complete (5.2GB)"
    exit 0
else
    CUR=$(stat -f%z "$FILE" 2>/dev/null || echo 0)
    PCT=$((CUR * 100 / EXPECTED))
    echo "📥 Downloading: ${PCT}%"
    exit 1
fi
```

This makes `pilot watchers` show: `gemma4-download [running] custom — 📥 Downloading: 42%`

## Available Watcher Types

### 1. Pipeline Watcher
Monitors an AzDo build until it completes (succeeds, fails, or is canceled).

```bash
pilot watch pipeline --name pipeline-pr567 --id <BUILD_ID> --interval 120
```

**How to find the build ID**: Use the AzDo REST API or extract from the pipeline URL (`buildId=XXXXX`).

### 2. PR Merge Watcher
Monitors a PR until it's merged (completed) or abandoned.

```bash
pilot watch pr-merge --name pr-merge-890 --id <PR_ID> --interval 120
```

### 3. PR Comments Watcher
Monitors a PR for new review comment threads. Notifies when new threads appear. Auto-stops when the PR is closed.

```bash
pilot watch pr-comments --name pr-comments-890 --id <PR_ID> --interval 180
```

### 4. SonarQube Watcher
Monitors a SonarQube quality gate until it resolves (OK or ERROR).

```bash
pilot watch sonarqube --name sonar-auth-svc --project-key <KEY> [--id <PR_ID>] --interval 120
```

Requires `SONARQUBE_URL` and `SONAR_TOKEN` environment variables.

### 5. Custom Script Watcher
Runs any bash script/command repeatedly.

**Ephemeral** (default): exit 0 = condition met → notify + auto-delete.
**Persistent** (`--persistent`): keeps running forever, notifies on state transitions only.

```bash
# Ephemeral: notify when download finishes, then disappear
pilot watch custom --name gemma4-download --script "./check-download.sh" --interval 60

# Persistent: monitor API health forever, notify on state changes
pilot watch custom --name api-health --persistent --script "curl -sf https://api.example.com/health" --interval 60
```

## Management Commands

```bash
# List all active watchers (🔄 persistent, ⚡ ephemeral)
pilot watchers

# Stop a watcher by name
pilot watchers --stop gemma4-download

# Clean up dead watcher entries
pilot watchers --cleanup

# Interactive TUI (prefix+W) — navigate, stop, delete, restart
pilot watchers --tui
```

### TUI Keybindings (prefix+W)
| Key | Action |
|-----|--------|
| `j/k` | Navigate |
| `s` | Stop selected |
| `d` | Delete from DB |
| `R` | Restart stopped watcher |
| `X` | Cleanup dead entries |
| `q` | Quit |

## Notification Flow

All watchers use `pilot notify` internally:
1. Watcher detects event → inserts notification into SQLite
2. tmux status bar refreshes → shows 🔔 count
3. If `notify.native = true` → macOS/Windows/Linux native notification fires
4. User presses `prefix+N` → sees notification center

## Usage Pattern

When the user says something like "watch the pipeline for this PR":

1. Determine the watcher type needed
2. Choose a descriptive `--name` based on context
3. Find the relevant ID (build ID, PR ID, etc.) — ask the user if not obvious
4. Run the `pilot watch` command via bash (it auto-detaches to background)
5. Confirm to the user that the watcher is running

Example interaction:
- User: "vigila el pipeline del PR 12345"
- Agent: runs `pilot watch pipeline --name pipeline-pr12345 --id <build_id> --interval 120`
- Agent: "✓ Pipeline watcher `pipeline-pr12345` started. You'll get a 🔔 when it completes."

## Important Notes

- **Ephemeral watchers** auto-delete from DB on completion — no cleanup needed
- **Persistent watchers** survive tmux/system restarts — auto-resurrected on startup via `pilot resurrect-watchers`
- Persistent watchers notify on **state transitions only** (OK→FAIL, FAIL→OK) to prevent notification spam
- Watchers run as detached background processes — they survive even if the copilot session exits
- `AZURE_DEVOPS_PAT` must be available in the environment for AzDo watchers
- Default poll interval is 120 seconds — increase for less urgent checks
- Use `--foreground` flag for debugging a watcher
