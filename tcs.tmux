#!/usr/bin/env bash
# tmux-copilot-sessions TPM plugin entry point
# Installs keybindings for tcs

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TCS_BIN="${CURRENT_DIR}/target/release/tcs"

# Fall back to PATH if binary not in plugin dir
if [ ! -x "$TCS_BIN" ]; then
    TCS_BIN="$(command -v tcs 2>/dev/null)"
fi

if [ -z "$TCS_BIN" ]; then
    tmux display-message "tcs: binary not found. Run 'cargo build --release' in ${CURRENT_DIR}"
    exit 1
fi

# Forward critical env vars into tmux server so display-popup inherits them
for var in AZURE_DEVOPS_PAT SIBA_PROJECT_BACKLOG SIBA_TEAM_NAME SIBA_AREA_PATH SIBA_CODE_PATH; do
    val=$(tmux show-environment "$var" 2>/dev/null)
    if [ $? -ne 0 ] && [ -n "${!var}" ]; then
        tmux set-environment "$var" "${!var}"
    fi
done

# Read keybindings from config or use defaults
FEATURE_KEY=$(tmux show-option -gqv @tcs-feature-key)
TASK_KEY=$(tmux show-option -gqv @tcs-task-key)
DASH_KEY=$(tmux show-option -gqv @tcs-dash-key)

FEATURE_KEY="${FEATURE_KEY:-F}"
TASK_KEY="${TASK_KEY:-T}"
DASH_KEY="${DASH_KEY:-D}"

# Bind keys using tmux display-popup for floating overlay
tmux bind-key "$FEATURE_KEY" display-popup -E -w 80% -h 80% "$TCS_BIN open"
tmux bind-key "$TASK_KEY" display-popup -E -w 80% -h 80% "$TCS_BIN task"
tmux bind-key "$DASH_KEY" display-popup -E -w 80% -h 80% "$TCS_BIN dash"
