# VHS tape for tmux-pilot demo GIF
# Run with: vhs docs/demo.tape

Output docs/screenshots/demo.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 14
Set Width 1200
Set Height 700
Set Padding 20
Set Theme "Builtin Dark"
Set TypingSpeed 60ms
Set WindowBar Colorful
Set WindowBarSize 40

# ─── Feature Selector ───────────────────────────────────────────────
Type "# Feature Selector — organize tmux sessions around work items"
Enter
Sleep 800ms

Type "pilot --demo"
Enter
Sleep 2s

# Navigate down through the list
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

# Navigate back up
Up
Sleep 400ms
Up
Sleep 400ms
Up
Sleep 400ms

Sleep 1.5s

# Quit feature selector
Type "q"
Sleep 1s

# ─── Task Selector ──────────────────────────────────────────────────
Type "# Task Selector — bugs, stories, tasks for a feature"
Enter
Sleep 800ms

Type "pilot task --demo"
Enter
Sleep 2s

# Navigate through tasks
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 400ms

Up
Sleep 400ms
Up
Sleep 400ms
Up
Sleep 400ms

Sleep 1.5s

Type "q"
Sleep 1s

# ─── Dashboard ───────────────────────────────────────────────────────
Type "# Dashboard — overview of all sessions"
Enter
Sleep 800ms

Type "pilot dash --demo"
Enter
Sleep 2s

Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms

Sleep 1.5s

Type "q"
Sleep 1s

# ─── Help ─────────────────────────────────────────────────────────
Type "# Full reference"
Enter
Sleep 800ms

Type "pilot help-all"
Enter
Sleep 3s

Sleep 500ms
