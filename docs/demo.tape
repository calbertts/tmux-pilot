# VHS tape for tmux-pilot demo GIF
# Records the plugin running inside tmux as popup modals
# Run with: vhs docs/demo.tape

Output docs/screenshots/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 18
Set Width 1100
Set Height 700
Set Padding 12
Set Theme "Builtin Dark"
Set TypingSpeed 50ms
Set WindowBar Colorful
Set WindowBarSize 40

# Start a demo tmux server with custom keybindings for --demo mode
Type `tmux -L demo new-session -s demo \; bind-key F display-popup -E -w 80% -h 80% 'pilot --demo' \; bind-key T display-popup -E -w 80% -h 80% 'pilot task --demo' \; bind-key D display-popup -E -w 80% -h 80% 'pilot dash --demo'`
Enter
Sleep 1.5s

# ─── Feature Selector (prefix+F) ────────────────────────────────────
Type "echo '🚀 prefix+F → Feature selector'"
Enter
Sleep 1s

Ctrl+b
Sleep 200ms
Type "F"
Sleep 2.5s

Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Up
Sleep 350ms
Up
Sleep 350ms
Up
Sleep 350ms

Sleep 1.5s
Type "q"
Sleep 1s

# ─── Task Selector (prefix+T) ───────────────────────────────────────
Type "echo '📋 prefix+T → Task selector'"
Enter
Sleep 800ms

Ctrl+b
Sleep 200ms
Type "T"
Sleep 2.5s

Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Up
Sleep 350ms
Up
Sleep 350ms

Sleep 1.5s
Type "q"
Sleep 1s

# ─── Dashboard (prefix+D) ───────────────────────────────────────────
Type "echo '📊 prefix+D → Dashboard'"
Enter
Sleep 800ms

Ctrl+b
Sleep 200ms
Type "D"
Sleep 2.5s

Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

Sleep 1.5s
Type "q"
Sleep 1s

# ─── Help ─────────────────────────────────────────────────────────
Type "pilot help-all"
Enter
Sleep 3s

# Clean up
Type "tmux -L demo kill-server"
Enter
Sleep 500ms
