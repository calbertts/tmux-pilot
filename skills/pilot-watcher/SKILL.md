---
name: pilot-watcher
description: Launch background watchers via pilot to monitor pipelines, PRs, SonarQube gates, and custom events with notifications.
---

# Pilot Watcher Skill

Launch background watchers that monitor events and push notifications via `pilot notify`. Notifications appear in the tmux status bar (🔔) and optionally as native OS notifications.

## When to Use

Activate this skill when the user asks to:
- Watch/monitor a pipeline, build, or CI run
- Get notified when a PR is merged, abandoned, or has new comments
- Monitor SonarQube quality gates
- Set up any recurring background check

## Naming Convention (REQUIRED)

**Always use `--name`** to give every watcher a human-readable identifier. The name should be short, descriptive, and use kebab-case:

| Context | Name example |
|---------|-------------|
| Pipeline for PR #567 | `--name pipeline-pr567` |
| PR merge watch #890 | `--name pr-merge-890` |
| PR comments watch | `--name pr-comments-890` |
| SonarQube for a service | `--name sonar-auth-service` |
| Custom: file download | `--name gemma4-download` |
| Custom: API health check | `--name api-health` |
| Custom: disk space monitor | `--name disk-usage-check` |

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
Runs any bash script/command repeatedly. When it exits with code 0, the notification fires. First line of stdout becomes the notification title. On non-zero exit, first line of stdout is saved as progress output.

```bash
pilot watch custom --name api-health --script "curl -sf https://api.example.com/health" --interval 60
```

## Management Commands

```bash
# List all active watchers (shows progress output for custom watchers)
pilot watchers

# Stop a watcher by name
pilot watchers --stop gemma4-download

# Clean up dead watcher entries
pilot watchers --cleanup
```

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

- Watchers run as detached background processes — they survive even if the copilot session exits
- Each watcher self-terminates when its condition resolves
- `AZURE_DEVOPS_PAT` must be available in the environment (inherited from tmux server via pilot.tmux)
- Default poll interval is 120 seconds — increase for less urgent checks
- Use `--foreground` flag for debugging a watcher
