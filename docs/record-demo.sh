#!/bin/bash
# Fully automated demo recording: Kitty window + tmux popups + screencapture
# Usage: ./docs/record-demo.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
SOCKET="pilot-demo"
SESSION="demo"
VIDEO="/tmp/pilot-demo.mov"
GIF="${REPO_DIR}/docs/screenshots/demo.gif"

cleanup() {
    tmux -L "$SOCKET" kill-server 2>/dev/null || true
    [ -n "$RECORD_PID" ] && kill "$RECORD_PID" 2>/dev/null || true
    [ -n "$KITTY_PID" ] && kill "$KITTY_PID" 2>/dev/null || true
    rm -f "$INNER_SCRIPT"
}
trap cleanup EXIT

# ─── 1. Prepare tmux demo server ─────────────────────────────────────
tmux -L "$SOCKET" kill-server 2>/dev/null || true

INNER_SCRIPT=$(mktemp /tmp/pilot-demo-XXXXXX.sh)
cat > "$INNER_SCRIPT" << 'INNEREOF'
#!/bin/bash
tmux -L pilot-demo -f /dev/null new-session -d -s demo
tmux -L pilot-demo set -g status-style "bg=#3c3836,fg=#ebdbb2"
tmux -L pilot-demo set -g status-left "#[fg=#282828,bg=#fabd2f,bold] demo #[fg=#fabd2f,bg=#3c3836] "
tmux -L pilot-demo set -g status-right ""
tmux -L pilot-demo set -g popup-border-style "fg=#fabd2f"
tmux -L pilot-demo attach -t demo
INNEREOF
chmod +x "$INNER_SCRIPT"

# ─── 2. Open Kitty window ────────────────────────────────────────────
kitty --title "pilot-demo" \
  -o font_size=18 \
  -o remember_window_size=no \
  -o initial_window_width=110c \
  -o initial_window_height=30c \
  -o macos_quit_when_last_window_closed=no \
  -o confirm_os_window_close=0 \
  "$INNER_SCRIPT" &
KITTY_PID=$!
sleep 3

# ─── 3. Get the Kitty window ID for screencapture ────────────────────
WINID=$(osascript -e '
    tell application "System Events"
        set kProcs to every process whose name is "kitty"
        repeat with p in kProcs
            set ws to windows of p
            repeat with w in ws
                if name of w contains "pilot-demo" then
                    return id of w
                end if
            end repeat
        end repeat
    end tell
' 2>/dev/null || true)

echo "Kitty window ID: ${WINID:-not found, will record full screen}"

# ─── 4. Set up background content ────────────────────────────────────
tmux -L "$SOCKET" send-keys "clear" Enter
sleep 0.3
tmux -L "$SOCKET" send-keys "echo ''" Enter
tmux -L "$SOCKET" send-keys "echo '   tmux-pilot'" Enter
tmux -L "$SOCKET" send-keys "echo ''" Enter
tmux -L "$SOCKET" send-keys "echo '   AI coding session manager for tmux'" Enter
tmux -L "$SOCKET" send-keys "echo ''" Enter
tmux -L "$SOCKET" send-keys "echo '   prefix+F  Feature selector'" Enter
tmux -L "$SOCKET" send-keys "echo '   prefix+T  Task selector'" Enter
tmux -L "$SOCKET" send-keys "echo '   prefix+D  Dashboard'" Enter
tmux -L "$SOCKET" send-keys "echo ''" Enter
sleep 1.5

# ─── 5. Start recording ──────────────────────────────────────────────
rm -f "$VIDEO"
if [ -n "$WINID" ]; then
    screencapture -v -l "$WINID" "$VIDEO" &
else
    screencapture -v "$VIDEO" &
fi
RECORD_PID=$!
sleep 1.5

# ─── 6. Run the demo (self-animating popups) ─────────────────────────
echo "Feature Selector popup"
tmux -L "$SOCKET" display-popup -t "$SESSION" -E -w 80% -h 80% "pilot --demo-auto"
sleep 1

echo "Task Selector popup"
tmux -L "$SOCKET" display-popup -t "$SESSION" -E -w 80% -h 80% "pilot task --demo-auto"
sleep 1

echo "Dashboard popup"
tmux -L "$SOCKET" display-popup -t "$SESSION" -E -w 80% -h 80% "pilot dash --demo-auto"
sleep 1.5

# ─── 7. Stop recording ───────────────────────────────────────────────
kill "$RECORD_PID" 2>/dev/null || true
RECORD_PID=""
sleep 1

echo "Converting to GIF..."
ffmpeg -y -i "$VIDEO" \
  -vf "fps=12,scale=1100:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3" \
  -loop 0 "$GIF" 2>/dev/null

rm -f "$VIDEO"

echo "GIF saved to: $GIF"
ls -lh "$GIF"
