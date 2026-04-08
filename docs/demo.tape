# VHS tape for tmux-pilot demo GIF
# Shows the TUI as floating popups inside tmux (display-popup)
# Run with: vhs docs/demo.tape

Output docs/screenshots/demo.gif

Set Shell "bash"
Set FontFamily "Menlo"
Set FontSize 18
Set Width 1100
Set Height 700
Set Padding 12
Set Theme "Builtin Dark"
Set TypingSpeed 40ms
Set WindowBar Colorful
Set WindowBarSize 40

# Start tmux with a clean demo server
Hide
Type "tmux -L pilot-demo new-session -s demo"
Enter
Sleep 1.5s
# Put some content in the background so the popup floats over it
Type "echo '  tmux-pilot — AI session manager'; echo ''; echo '  prefix+F  Feature selector'; echo '  prefix+T  Task selector'; echo '  prefix+D  Dashboard'; echo '  prefix+N  Notifications'; echo '  prefix+W  Watchers'; echo ''"
Enter
Sleep 500ms
Show
Sleep 1.5s

# ─── Feature Selector popup ─────────────────────────────────────────
Hide
Type "tmux display-popup -E -w 80% -h 80% 'pilot --demo'"
Enter
Sleep 2s
Show
Sleep 1s

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

Sleep 2s
Type "q"
Sleep 1s

# ─── Task Selector popup ────────────────────────────────────────────
Hide
Type "tmux display-popup -E -w 80% -h 80% 'pilot task --demo'"
Enter
Sleep 2s
Show
Sleep 1s

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

Sleep 2s
Type "q"
Sleep 1s

# ─── Dashboard popup ────────────────────────────────────────────────
Hide
Type "tmux display-popup -E -w 80% -h 80% 'pilot dash --demo'"
Enter
Sleep 2s
Show
Sleep 1s

Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

Sleep 2s
Type "q"
Sleep 1.5s

# Clean up
Hide
Type "tmux -L pilot-demo kill-server"
Enter
Sleep 500ms
