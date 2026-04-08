#!/usr/bin/env bash
# tmux-pilot TPM plugin entry point
# Installs keybindings and auto-downloads binary if needed

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PILOT_BIN="${CURRENT_DIR}/bin/pilot"

# Fall back to cargo build output
if [ ! -x "$PILOT_BIN" ]; then
    PILOT_BIN="${CURRENT_DIR}/target/release/pilot"
fi

# Fall back to PATH
if [ ! -x "$PILOT_BIN" ]; then
    PILOT_BIN="$(command -v pilot 2>/dev/null)"
fi

# Auto-install from GitHub Releases if not found
if [ -z "$PILOT_BIN" ] || [ ! -x "$PILOT_BIN" ]; then
    tmux display-message "pilot: downloading binary..."
    if bash "${CURRENT_DIR}/scripts/install.sh" "${CURRENT_DIR}/bin" >/dev/null 2>&1; then
        PILOT_BIN="${CURRENT_DIR}/bin/pilot"
        tmux display-message "pilot: installed successfully ✓"
    else
        tmux display-message "pilot: auto-install failed. Run: cd ${CURRENT_DIR} && cargo build --release"
        exit 1
    fi
fi

# Forward critical env vars into tmux server so display-popup inherits them
for var in AZURE_DEVOPS_PAT PILOT_AZDO_PROJECT PILOT_AZDO_TEAM PILOT_AZDO_AREA PILOT_CODE_PATH; do
    val=$(tmux show-environment "$var" 2>/dev/null)
    if [ $? -ne 0 ] && [ -n "${!var}" ]; then
        tmux set-environment "$var" "${!var}"
    fi
done

# Read keybindings from config or use defaults
FEATURE_KEY=$(tmux show-option -gqv @pilot-feature-key)
TASK_KEY=$(tmux show-option -gqv @pilot-task-key)
DASH_KEY=$(tmux show-option -gqv @pilot-dash-key)

FEATURE_KEY="${FEATURE_KEY:-F}"
TASK_KEY="${TASK_KEY:-T}"
DASH_KEY="${DASH_KEY:-D}"

# Read notification key (default: N)
NOTIFY_KEY=$(tmux show-option -gqv @pilot-notify-key)
NOTIFY_KEY="${NOTIFY_KEY:-N}"

# Read watcher key (default: W)
WATCHER_KEY=$(tmux show-option -gqv @pilot-watcher-key)
WATCHER_KEY="${WATCHER_KEY:-W}"

# Bind keys using tmux display-popup for floating overlay
tmux bind-key "$FEATURE_KEY" display-popup -E -w 80% -h 80% "$PILOT_BIN open"
tmux bind-key "$TASK_KEY" display-popup -E -w 80% -h 80% "$PILOT_BIN task"
tmux bind-key "$DASH_KEY" display-popup -E -w 80% -h 80% "$PILOT_BIN dash"
tmux bind-key "$NOTIFY_KEY" display-popup -E -w 80% -h 60% "$PILOT_BIN notifications"
tmux bind-key "$WATCHER_KEY" display-popup -E -w 80% -h 60% "$PILOT_BIN watchers --tui"

# Inject notification count into status-right (prepend to existing)
CURRENT_STATUS_RIGHT=$(tmux show-option -gqv status-right)
NOTIF_SEGMENT="#($PILOT_BIN notifications --count --format tmux)"
# Only inject if not already present
if [[ "$CURRENT_STATUS_RIGHT" != *"pilot notifications"* ]]; then
    tmux set-option -g status-right "${NOTIF_SEGMENT}${CURRENT_STATUS_RIGHT}"
fi

# Auto-restore copilot sessions after tmux server restart.
# Uses tmux server PID as marker — only runs once per server lifetime.
TMUX_PID=$(tmux display-message -p '#{pid}')
RESTORE_MARKER="/tmp/pilot-restored-${TMUX_PID}"
if [ ! -f "$RESTORE_MARKER" ]; then
    touch "$RESTORE_MARKER"
    # Delay to let tmux-resurrect finish restoring sessions first
    tmux run-shell -b "sleep 5 && $PILOT_BIN restore 2>/dev/null"
fi
