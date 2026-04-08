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

## Available Watcher Types

### 1. Pipeline Watcher
Monitors an AzDo build until it completes (succeeds, fails, or is canceled).

```bash
pilot watch pipeline --id <BUILD_ID> --interval 120
```

**How to find the build ID**: Use the AzDo REST API or extract from the pipeline URL (`buildId=XXXXX`).

### 2. PR Merge Watcher
Monitors a PR until it's merged (completed) or abandoned.

```bash
pilot watch pr-merge --id <PR_ID> --interval 120
```

### 3. PR Comments Watcher
Monitors a PR for new review comment threads. Notifies when new threads appear. Auto-stops when the PR is closed.

```bash
pilot watch pr-comments --id <PR_ID> --interval 180
```

### 4. SonarQube Watcher
Monitors a SonarQube quality gate until it resolves (OK or ERROR).

```bash
pilot watch sonarqube --project-key <KEY> [--id <PR_ID>] --interval 120
```

Requires `SONARQUBE_URL` and `SONAR_TOKEN` environment variables.

### 5. Custom Script Watcher
Runs any bash script/command repeatedly. When it exits with code 0, the notification fires. First line of stdout becomes the notification title.

```bash
pilot watch custom --script "curl -s https://api.example.com/status | grep -q 'ready'" --interval 60
```

## Management Commands

```bash
# List all active watchers
pilot watchers

# Stop a specific watcher
pilot watchers --stop <WATCHER_ID>

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
2. Find the relevant ID (build ID, PR ID, etc.) — ask the user if not obvious
3. Run the `pilot watch` command via bash (it auto-detaches to background)
4. Confirm to the user that the watcher is running

Example interaction:
- User: "vigila el pipeline del PR 12345"
- Agent: runs `pilot watch pipeline --id <build_id> --interval 120`
- Agent: "✓ Pipeline watcher started (pid: XXXX). You'll get a 🔔 notification when it completes."

## Important Notes

- Watchers run as detached background processes — they survive even if the copilot session exits
- Each watcher self-terminates when its condition resolves
- `AZURE_DEVOPS_PAT` must be available in the environment (inherited from tmux server via pilot.tmux)
- Default poll interval is 120 seconds — increase for less urgent checks
- Use `--foreground` flag for debugging a watcher
