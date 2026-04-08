# VHS tape for tmux-pilot demo GIF
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

# ─── Feature Selector ────────────────────────────────────────────────
Hide
Type "clear"
Enter
Show
Sleep 300ms
Type "pilot --demo"
Enter
Sleep 2s

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
Sleep 500ms

# ─── Task Selector ───────────────────────────────────────────────────
Hide
Type "clear"
Enter
Show
Sleep 300ms
Type "pilot task --demo"
Enter
Sleep 2s

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
Sleep 500ms

# ─── Dashboard ───────────────────────────────────────────────────────
Hide
Type "clear"
Enter
Show
Sleep 300ms
Type "pilot dash --demo"
Enter
Sleep 2s

Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

Sleep 2s
Type "q"
Sleep 500ms

# ─── Help ────────────────────────────────────────────────────────────
Hide
Type "clear"
Enter
Show
Sleep 300ms
Type "pilot help-all"
Enter
Sleep 4s
